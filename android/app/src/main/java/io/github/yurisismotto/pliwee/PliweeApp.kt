package io.github.yurisismotto.pliwee

import android.app.Application
import android.util.Log
import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.CapabilityRegistry
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.capability.NotificationsCapability
import io.github.yurisismotto.pliwee.capability.SensitiveCapabilities
import io.github.yurisismotto.pliwee.clipboard.ClipboardNotifications
import io.github.yurisismotto.pliwee.clipboard.ClipboardSync
import io.github.yurisismotto.pliwee.proto.Platform
import io.github.yurisismotto.pliwee.clipboard.SystemClipboard
import io.github.yurisismotto.pliwee.files.FileTransferManager
import io.github.yurisismotto.pliwee.identity.DeviceIdentity
import io.github.yurisismotto.pliwee.net.ConnectResult
import io.github.yurisismotto.pliwee.net.Discovery
import io.github.yurisismotto.pliwee.net.Endpoints
import io.github.yurisismotto.pliwee.net.PeerConnection
import io.github.yurisismotto.pliwee.net.PeerProfiles
import io.github.yurisismotto.pliwee.notifications.KeyguardLockState
import io.github.yurisismotto.pliwee.notifications.NotificationAccess
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.notifications.NotificationSecret
import io.github.yurisismotto.pliwee.notifications.NotificationSource
import io.github.yurisismotto.pliwee.pairing.QrPayload
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import java.net.InetSocketAddress
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Application-scoped wiring.
 *
 * Deliberately plain: no dependency-injection framework for six objects.
 */
class PliweeApp : Application() {

    lateinit var trustStore: TrustStore
        private set
    lateinit var identity: DeviceIdentity
        private set
    lateinit var battery: BatteryCapability
        private set
    lateinit var registry: CapabilityRegistry
        private set
    lateinit var discovery: Discovery
        private set
    lateinit var files: FileTransferManager
        private set
    lateinit var clipboard: ClipboardSync
        private set
    lateinit var systemClipboard: SystemClipboard
        private set

    /**
     * `notifications.v1` — the Android source adapter.
     *
     * Held here rather than inside the listener service because the service is
     * created and destroyed by the system as it binds and unbinds, and the
     * capability's state — role epochs, the id map, the outbound queue —
     * belongs to the app rather than to one binding.
     */
    lateinit var notifications: NotificationSource
        private set

    /**
     * Application-lifetime scope for file transfers.
     *
     * Not the connection service's scope: a transfer must survive the UI
     * being closed, and must not be cancelled by a screen rotation.
     */
    private val appScope = kotlinx.coroutines.CoroutineScope(
        kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Default,
    )

    private val _connectionState = MutableStateFlow<ConnectionState>(ConnectionState.Idle)
    val connectionState: StateFlow<ConnectionState> = _connectionState.asStateFlow()

    sealed interface ConnectionState {
        data object Idle : ConnectionState
        data object Connecting : ConnectionState
        data class Connected(val deviceName: String, val fingerprintShort: String) :
            ConnectionState

        /**
         * Not connected, but a retry is scheduled. Distinct from [Error]:
         * this says the app is still trying, which is the difference the
         * reconnect defect made invisible.
         */
        data class Retrying(val reason: String, val inSeconds: Long) : ConnectionState
        data class Error(val message: String) : ConnectionState
    }

    /** Lets the connection service publish the state it owns. */
    fun publishConnectionState(state: ConnectionState) {
        _connectionState.value = state
    }

    /**
     * What the **live session** negotiated, if there is one.
     *
     * ## Why this is not read from the trust store
     *
     * A grant says what a computer is *allowed* to do. A negotiated capability
     * set says what the session that is up can *actually carry*, and the two
     * come apart: the set is fixed by `HELLO` and is the authority for the
     * whole life of that session, so a capability granted afterwards is not on
     * it until a new handshake runs (ADR-0017 §3). A screen that asked only
     * the trust store therefore offered a Send button over a session that
     * would drop the frame — which is half of GitHub #8.
     *
     * ## Why it is never persisted
     *
     * It is connection state, not trust. Writing it down would create a second
     * record of what a peer may do, ageing the moment the session ends, and
     * the whole point of asking the live session is that it cannot be stale.
     * It is cleared when the session ends, and a process restart starts at
     * null rather than at a remembered yes.
     */
    data class LiveSession(
        /** Whose session this is. The routing identity, never a name. */
        val peerHex: String,
        /** Capability ids this session negotiated. Not grants. */
        val negotiated: Set<String>,
        /**
         * What the peer said it runs, from the HELLO this session ran.
         *
         * `DeviceInfo.platform` on the wire, carried here rather than into
         * the trust store on purpose: a stored platform would be a claim
         * about a machine that is not talking to us, made by a message that
         * arrived days ago. Held for the life of the session, it is a fact
         * about the device on the other end right now.
         *
         * `PLATFORM_UNSPECIFIED` from an implementation that does not set it
         * stays unspecified. Nothing guesses.
         */
        val platform: Platform = Platform.PLATFORM_UNSPECIFIED,
    )

    private val _liveSession = MutableStateFlow<LiveSession?>(null)
    val liveSession: StateFlow<LiveSession?> = _liveSession.asStateFlow()

    /** Called by the connection service when a session comes up or goes. */
    fun publishLiveSession(session: LiveSession?) {
        _liveSession.value = session
    }

    override fun onCreate() {
        super.onCreate()
        trustStore = TrustStore(this)
        identity = DeviceIdentity.loadOrCreate(trustStore.deviceId)
        // Same rule as files and clipboard below: the grant is re-read on every
        // question, so the Battery switch on the device card is what decides.
        battery = BatteryCapability(this) { peer -> isBatterySharingAllowed(peer) }

        // The grant is re-read from the trust store on every question rather
        // than captured once, so revoking `files.v1` — or forgetting the
        // computer entirely — takes effect immediately, including against a
        // transfer that is already running.
        files = FileTransferManager(
            context = this,
            identity = identity,
            scope = appScope,
            isAuthorized = { peer -> isFileTransferAllowed(peer) },
        )

        // clipboard.v1. The grant and the per-peer policy are both re-read
        // from the trust store on every question, so forgetting a computer —
        // or withdrawing just its clipboard grant — stops traffic at once,
        // including on a session that is already connected.
        systemClipboard = SystemClipboard(this)
        clipboard = ClipboardSync(
            systemClipboard = systemClipboard,
            authorizer = { peer -> clipboardPolicyFor(peer) },
            localDeviceId = trustStore.deviceId,
        )

        // With `autoReceive` off — the default — an accepted clip is held in
        // memory and offered here. The notification carries no clipboard
        // text, only the computer's name and the size.
        val clipPending = ClipboardNotifications(this)
        clipboard.onClipPending = clipPending::show

        // notifications.v1. Three permissions, checked independently and none
        // of them implying another: Android's notification access, this peer's
        // grant in the trust store, and the roles each side announces. The
        // secret lives in the Keystore and is resolved lazily, so a keystore
        // that is briefly unavailable makes the capability inert for that
        // session rather than replacing a live secret.
        notifications = NotificationSource(
            ownPackage = packageName,
            localDeviceId = trustStore.deviceId,
            authorizer = { peer -> notificationPolicyFor(peer) },
            appLabels = ::appLabelFor,
            secretProvider = ::notificationSecret,
            lockState = KeyguardLockState(this),
            access = NotificationAccess.controlFor(this),
        )
        notifications.start(appScope)

        registry = CapabilityRegistry(
            listOf(
                battery,
                FilesCapability(files),
                ClipboardCapability(clipboard) { peer ->
                    trustStore.peer(peer)?.deviceName ?: "a paired computer"
                },
                NotificationsCapability(notifications),
            ),
        )
        discovery = Discovery(this)

        Log.i(
            TAG,
            "identity ${identity.fingerprint.toDisplayShort()} " +
                "(strongbox=${identity.isStrongBoxBacked})",
        )
    }

    /** Whether this phone's battery may be sent to a computer, right now. */
    private fun isBatterySharingAllowed(peer: io.github.yurisismotto.pliwee.identity.Fingerprint):
        Boolean = runCatching {
        trustStore.peer(peer)?.allows(BatteryCapability.ID) == true
    }.getOrDefault(false)

    /**
     * Whether a computer may transfer files with this phone, right now.
     *
     * Answered from the trust store every time. A peer that was forgotten, or
     * whose `files.v1` grant was withdrawn, stops being authorized at once —
     * the handshake's answer is a snapshot and would not.
     */
    private fun isFileTransferAllowed(peer: io.github.yurisismotto.pliwee.identity.Fingerprint):
        Boolean = runCatching {
        trustStore.peer(peer)?.grantedCapabilities?.contains(FilesCapability.ID) == true
    }.getOrDefault(false)

    /**
     * What a computer may do with this phone's clipboard, right now.
     *
     * Failing closed on any error is deliberate: an unreadable trust store is
     * a reason to permit nothing, not a reason to permit everything.
     */
    private fun clipboardPolicyFor(
        peer: io.github.yurisismotto.pliwee.identity.Fingerprint,
    ): io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy = runCatching {
        trustStore.clipboardPolicyFor(peer)
    }.getOrDefault(io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy.DENIED)

    /**
     * What a computer may be told about this phone's notifications, right now.
     *
     * Failing closed on any error, for the same reason [clipboardPolicyFor]
     * does: an unreadable trust store is a reason to permit nothing.
     */
    private fun notificationPolicyFor(
        peer: io.github.yurisismotto.pliwee.identity.Fingerprint,
    ): NotificationPolicy = runCatching {
        trustStore.notificationPolicyFor(peer)
    }.getOrDefault(NotificationPolicy.DENIED)

    /**
     * `device_notification_secret`, resolved on demand.
     *
     * Cached once it is available, because it does not change while the
     * process lives; **not** cached as "absent", so a keystore that could not
     * answer once is asked again rather than disabling the capability for the
     * life of the process. Nothing here can return the key material: the
     * secret is a non-exportable Keystore HMAC key and this returns a handle
     * that can only be used to compute a MAC.
     */
    @Volatile
    private var cachedNotificationSecret: NotificationSecret? = null

    private fun notificationSecret(): NotificationSecret? {
        cachedNotificationSecret?.let { return it }
        val secret = NotificationSecret.loadOrCreate(
            NotificationSecret.KeystoreSecretStore(),
            trustStore,
        )
        if (secret != null) {
            // A state name and a generation count. No key material, ever.
            Log.i(TAG, "notification secret ${secret.state} generation=${secret.generation}")
            cachedNotificationSecret = secret
        }
        return secret
    }

    /**
     * A package name resolved to something a person recognises.
     *
     * Only the desktop's alternative would be a package database it does not
     * have, so the label is resolved here — off the listener callback thread,
     * because this is a binder call. A package that cannot be resolved falls
     * back to its own name rather than to an empty string: "com.example.chat"
     * is worse than "Chat" and much better than nothing.
     *
     * This needs no `QUERY_ALL_PACKAGES`: an app that just posted a
     * notification to our listener is visible to us, and a package that is not
     * resolvable simply keeps its id.
     */
    private fun appLabelFor(packageName: String): String = runCatching {
        val info = packageManager.getApplicationInfo(packageName, 0)
        packageManager.getApplicationLabel(info).toString()
    }.getOrDefault(packageName)

    /**
     * Which computer this phone is connecting to, right now.
     *
     * The single answer to "where to", read fresh every time from the trust
     * store and the person's choice. It replaces `peers().firstOrNull()` —
     * the certified U2 §39.17 defect, where storage order was the routing
     * table and an offline first entry made every later peer unreachable.
     *
     * Failing closed on a trust-store error, for the reason the capability
     * authorizers do: an unreadable store is a reason to dial nothing, not a
     * reason to dial whatever was cached.
     */
    fun targetResolution(): PeerTarget.Resolution = runCatching {
        PeerTarget.resolve(trustStore.peers(), trustStore.selectedPeerHex)
    }.getOrDefault(PeerTarget.Resolution.NoTrustedPeer)

    /** The one computer to dial, or null when there is no unambiguous one. */
    fun targetPeer(): TrustStore.TrustedPeer? = targetResolution().peerOrNull()

    /**
     * Records the person's choice of computer.
     *
     * Returns false — and changes nothing — for a fingerprint that is not
     * trusted. Choosing a destination is not a route to trust: an unpaired
     * computer still has to be paired, and the pin is still checked on every
     * connection to a chosen one.
     */
    fun selectPeer(fingerprint: io.github.yurisismotto.pliwee.identity.Fingerprint): Boolean =
        runCatching { trustStore.selectPeer(fingerprint) }.getOrDefault(false)

    /**
     * Where to try reaching a paired computer, best guess first.
     *
     * A peer is a *set* of possible endpoints, not one address. A dual-stack
     * daemon publishes several, and the phone may also remember where the
     * computer was last time. Taking the first of those is how the phone
     * ended up dialling a link-local IPv6 address that could never answer.
     *
     * `round` is the attempt number within the current reconnect cycle.
     * Round 1 uses the remembered addresses alone when there are any: they
     * usually work and cost nothing, whereas browsing costs a multicast lock
     * and several seconds. From round 2 — meaning the remembered ones just
     * failed — discovery runs and its results are merged in. The order within
     * the result is decided by [Endpoints.order].
     */
    suspend fun candidateAddresses(
        peer: TrustStore.TrustedPeer,
        round: Int = 1,
    ): List<InetSocketAddress> {
        val remembered = peer.dialAddresses()
        if (round <= 1 && remembered.isNotEmpty()) return remembered

        val found = discover(peer.deviceId) ?: return remembered
        // Which service type(s) announced it decides the profile it is
        // dialled with — for these addresses and the remembered ones alike.
        peerProfiles.learnedFromDiscovery(peer.fingerprint.toHex(), found.profile)
        // Not `remembered + found`: several remembered hints could then fill
        // the round ahead of the address discovery just saw. See
        // [Endpoints.withDiscovered].
        return Endpoints.withDiscovered(remembered, found.addresses)
    }

    /**
     * The profile each paired computer is dialled with. In memory only; see
     * [PeerProfiles] for the rule and for why it is never persisted.
     */
    val peerProfiles = PeerProfiles()

    /**
     * Collects every address advertised for [deviceId] within the discovery
     * window.
     *
     * Deliberately not `first { }`: one resolve callback delivers all of a
     * service's addresses, and keeping only one of them throws away the very
     * alternative that would have worked. The window is bounded and the
     * result is capped, so a hostile responder cannot make this run long or
     * return an unbounded list.
     */
    private suspend fun discover(deviceId: String): Discovery.Device? {
        // Collected into a list the timeout cannot take away: whatever was
        // found before the window closed is still worth dialling.
        val found = ArrayList<Discovery.Found>()
        withTimeoutOrNull(DISCOVERY_TIMEOUT_MS) {
            discovery.browse()
                // Discovery failing is "nothing found", never an exception
                // thrown at the reconnect loop. Belt and braces on top of
                // Discovery's own handling: this collector must not be the
                // thing that kills a reconnect job.
                .catch { e -> Log.w(TAG, "discovery stopped: ${e.javaClass.simpleName}") }
                // The device id is only a filter to avoid dialling unrelated
                // services. It is not identity: that is decided by the pinned
                // key during the TLS handshake.
                .filter { it.deviceId == null || it.deviceId == deviceId }
                // Completes the flow rather than cancelling anything, and
                // bounds the work a hostile responder can cause. Counted in
                // records: a Pliwee daemon announces itself under both
                // service types, so one device is up to twice as many.
                .take(MAX_DISCOVERED_ADDRESSES * WIRE_PROFILE_COUNT)
                .collect { found += it }
        }
        // One device, however many records: the two announcements of one
        // daemon merge, and the canonical one wins the profile.
        return Discovery.mergeFor(deviceId, found)?.let {
            it.copy(addresses = it.addresses.take(MAX_DISCOVERED_ADDRESSES))
        }
    }

    /**
     * Connects to a known peer using its persisted, pinned identity.
     *
     * Reports no state of its own: the connection service owns the link
     * state, because it is the only component that knows whether a failure is
     * about to be retried. Setting it here as well was how the UI came to
     * show "Connected" long after the session had died.
     */
    suspend fun connect(
        peer: TrustStore.TrustedPeer,
        address: InetSocketAddress,
    ): ConnectResult = PeerConnection.connect(
        address = address,
        identity = identity,
        deviceName = trustStore.deviceName,
        pinned = peer.fingerprint,
        registry = registry,
        profile = peerProfiles.forDialing(peer.fingerprint.toHex()),
    )

    /**
     * The computer a scanned code is currently proving a token for.
     *
     * Read by [ConnectionService] through [PairingGate] so the tokenless
     * reconnect dialer holds off for exactly as long as a pairing is in
     * flight, and for exactly that one computer. See [PairingGate] for the
     * defect this closes, and why it is a hold rather than a stop.
     *
     * A hex string rather than a `Fingerprint`, for the reason
     * `selectedPeerFlow` gives: `Fingerprint` wraps a `ByteArray` whose
     * `equals` is identity.
     */
    private val pairingInFlight = java.util.concurrent.atomic.AtomicReference<String?>(null)

    /** Null when no scanned code is being proved. */
    val pairingInFlightHex: String? get() = pairingInFlight.get()

    /**
     * How a scanned code resolved.
     *
     * [provedToken] is the difference between "this computer demanded the
     * QR's token and got it" and "this computer already trusted this phone
     * and asked for nothing". Both are legitimate answers to a scan — the
     * responder decides what it requires — and they are different events, so
     * they are not collapsed into one word on the screen.
     */
    data class PairOutcome(
        val peer: TrustStore.TrustedPeer,
        val provedToken: Boolean,
    )

    /**
     * Completes pairing from a scanned QR code.
     *
     * The desktop's fingerprint comes from the QR and is pinned before the
     * socket opens, so there is no window in which an impostor could answer.
     *
     * ## UX-DEBT-02 — a scan is an instruction about this computer
     *
     * Scanning is explicit, so for the length of the attempt this is the only
     * thing allowed to dial that computer ([pairingInFlight]). Without it the
     * reconnect coordinator's *tokenless* dial could reach a desktop waiting
     * for a pairing proof, stop at `PairingRequired` without sending one, and
     * be declared terminal — while the person was mid-way through re-pairing.
     *
     * Nothing about trust is decided here. The desktop decides whether a
     * proof is required, verifies it, and asks a human; this function only
     * makes sure the right connection is the one carrying the token, and
     * writes the result down without trampling settings that were already
     * there ([TrustStore.upsertPairedPeer]).
     *
     * On every failure path the trust store is left exactly as it was: no
     * record is created, none is deleted, no grant moves and the chosen
     * computer is unchanged. Trust is written in one place below, after the
     * session is established.
     */
    suspend fun pair(payload: QrPayload): Result<PairOutcome> {
        val targetHex = payload.fingerprint.toHex()
        // Also the guard against two scans at once: a second one finds the
        // slot taken rather than racing the first for the same window.
        if (!pairingInFlight.compareAndSet(null, targetHex)) {
            return Result.failure(
                IllegalStateException("a pairing code is already being proved"),
            )
        }
        try {
            return pairWithToken(payload, targetHex)
        } finally {
            pairingInFlight.set(null)
        }
    }

    private suspend fun pairWithToken(
        payload: QrPayload,
        targetHex: String,
    ): Result<PairOutcome> {
        _connectionState.value = ConnectionState.Connecting

        // The scanned scheme fixes the profile of this pairing (ADR-0020
        // §D4); discovery may help find the computer but never changes it.
        val addresses = Endpoints.order(
            payload.addresses.ifEmpty { discover(payload.deviceId)?.addresses.orEmpty() },
        )
        if (addresses.isEmpty()) {
            _connectionState.value = ConnectionState.Error("could not find the computer")
            return Result.failure(IllegalStateException("no reachable address"))
        }

        var lastError = "could not reach the computer"
        for (address in addresses) {
            val result = PeerConnection.connect(
                address = address,
                identity = identity,
                deviceName = trustStore.deviceName,
                pinned = payload.fingerprint,
                registry = registry,
                profile = payload.profile,
                pairingToken = payload.token,
            )
            when (result) {
                is ConnectResult.Established -> {
                    val connection = result.connection
                    val pairedAt = System.currentTimeMillis() / 1000
                    val peer = TrustStore.TrustedPeer(
                        deviceId = connection.peerDevice.deviceId,
                        deviceName = TrustStore.sanitizeDeviceName(
                            connection.peerDevice.deviceName,
                        ),
                        fingerprint = payload.fingerprint,
                        pairedAtUnix = pairedAt,
                        // What the desktop advertises is what both sides
                        // *support*, not what either has authorized. The
                        // sensitive capabilities — `clipboard.v1` and
                        // `notifications.v1` — are withheld here and turned on
                        // per peer from the device card; see
                        // [SensitiveCapabilities] for what makes each of them
                        // sensitive and why the set is named rather than
                        // subtracted inline, as it was here.
                        //
                        // This is no longer the only thing standing between a
                        // negotiation and a grant: `mergePairing` applies the
                        // same set where the union happens, so a re-pair
                        // cannot escalate even if this line is one day wrong.
                        grantedCapabilities = connection.negotiatedCapabilities
                            .toSet() - SensitiveCapabilities.NEVER_AUTO_GRANTED,
                        // The address this pinned session just came up at:
                        // a successful hint, because it is established.
                        addressHints = listOfNotNull(AddressHint.from(address, pairedAt)),
                    )
                    // Merged, not replaced: a computer paired again keeps
                    // the grants and policies the person set for it. See
                    // `TrustStore.upsertPairedPeer` — and note this is the
                    // only line in this function that writes trust.
                    val stored = trustStore.upsertPairedPeer(peer)
                    // Dial it back the way it was paired, until discovery
                    // says otherwise. Memory only: never part of trust.
                    peerProfiles.learnedFromPairing(targetHex, payload.profile)
                    connection.disconnect()
                    // `files.v1` is granted here because the person just
                    // paired this computer by hand, and every incoming file
                    // still needs their explicit approval. The grant is
                    // revocable from the device card, and revoking it takes
                    // effect immediately.
                    _connectionState.value = ConnectionState.Idle
                    return Result.success(
                        PairOutcome(stored, provedToken = result.provedPairing),
                    )
                }
                // Unreachable from here — a scanned payload always carries a
                // token, and this is what `PeerConnection` returns when there
                // is none to offer — but stated honestly rather than as "the
                // computer declined", which it never was.
                is ConnectResult.PairingRequired ->
                    lastError = "no pairing code was offered to the computer"
                is ConnectResult.Failed -> lastError = result.reason
            }
        }

        _connectionState.value = ConnectionState.Error(lastError)
        return Result.failure(IllegalStateException(lastError))
    }

    companion object {
        private const val TAG = "PliweeApp"
        private const val DISCOVERY_TIMEOUT_MS = 5_000L

        /**
         * Enough to cover a dual-stack daemon plus a spare. A cap is needed
         * because the responder is attacker-controlled and could otherwise
         * publish records until the phone ran out of patience.
         */
        private const val MAX_DISCOVERED_ADDRESSES = 6

        /** Service types browsed per device: one per wire profile. */
        private val WIRE_PROFILE_COUNT = io.github.yurisismotto.pliwee.net.WireProfile.entries.size
    }
}
