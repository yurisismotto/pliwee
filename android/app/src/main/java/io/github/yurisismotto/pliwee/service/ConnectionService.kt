package io.github.yurisismotto.pliwee.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import io.github.yurisismotto.pliwee.PliweeApp
import io.github.yurisismotto.pliwee.R
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.net.ConnectResult
import io.github.yurisismotto.pliwee.net.ConnectionCoordinator
import io.github.yurisismotto.pliwee.net.DialResult
import io.github.yurisismotto.pliwee.net.FailureKind
import io.github.yurisismotto.pliwee.net.LinkState
import io.github.yurisismotto.pliwee.net.PeerConnection
import io.github.yurisismotto.pliwee.pairing.PairingGate
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.MainActivity
import kotlinx.coroutines.launch

/**
 * Keeps a connection to the paired computer while one is wanted.
 *
 * ## Why `connectedDevice` and not `dataSync`
 *
 * `dataSync` is time-boxed by the platform and is meant for finite transfers;
 * using it as a permanent daemon is precisely the pattern Android has been
 * tightening down on, and it would be killed. `connectedDevice` is the type
 * intended for maintaining a link with an external device, which is literally
 * what this is.
 *
 * That type requires the app to hold a qualifying permission. We hold
 * `CHANGE_WIFI_MULTICAST_STATE` because we genuinely take a multicast lock for
 * mDNS discovery — the permission is not a formality to satisfy the check.
 *
 * ## Playing by the rules
 *
 * * The service runs only while the user has the connection enabled. It stops
 *   otherwise, rather than idling.
 * * Reconnection is capped exponential backoff with jitter, driven by
 *   [ConnectionCoordinator]. `ConnectivityManager` callbacks can bring a
 *   pending retry forward but are never the only thing that schedules one.
 *   There is no wakelock-and-spin loop, no alarm hammering, and nothing that
 *   fights Doze.
 * * No `START_STICKY` resurrection games and no boot receiver: if the system
 *   stops us, we stay stopped until the user brings us back.
 *
 * ## This class does not decide when to retry
 *
 * It owns the notification, the network callback and the platform lifecycle,
 * and it hands the coordinator two functions: where to dial and how. Keeping
 * the retry policy in one place — and out of the component that has four
 * different callback threads — is what fixes the loop that used to die.
 *
 * ## It does not decide *which computer* either
 *
 * It used to, and that was the U2 §39.17 defect: `peers().firstOrNull()` made
 * trust-store order the routing table, so a desktop that had been powered off
 * for a week kept every later peer unreachable. The destination now comes from
 * `PeerTarget`, re-read at the top of every round, and reaches this class as a
 * fingerprint in the start intent — an identity, not a position.
 */
class ConnectionService : LifecycleService() {

    private lateinit var connectivity: ConnectivityManager
    private var coordinator: ConnectionCoordinator? = null

    /** The session currently running, so a lost network can end it promptly. */
    @Volatile
    private var currentConnection: PeerConnection? = null

    /**
     * Whose session [currentConnection] is, as a fingerprint hex.
     *
     * Kept beside the connection rather than inferred from the trust store,
     * because the question it answers is "is the live session the one we still
     * want", and by the time that is asked the store already holds the *new*
     * choice. Without it, retargeting could not tell a session that must be
     * ended from one that must be left alone.
     */
    @Volatile
    private var currentSessionPeerHex: String? = null

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            coordinator?.onNetworkAvailable("wifi-or-ethernet")
        }

        override fun onLost(network: Network) {
            coordinator?.onNetworkLost("wifi-or-ethernet")
            // Closing the socket makes the blocked read fail now instead of
            // waiting out the idle timeout. It does not cancel anything: the
            // coordinator sees an ordinary session end and schedules a retry
            // exactly as it would for any other failure.
            currentConnection?.disconnect()
        }
    }

    override fun onCreate() {
        super.onCreate()
        connectivity = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        createNotificationChannel()
    }

    override fun onBind(intent: Intent): IBinder? {
        super.onBind(intent)
        return null
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)

        if (intent?.action == ACTION_STOP) {
            // An explicit stop is not a failure and must never be retried.
            coordinator?.stop("user asked to disconnect")
            stopSelf()
            return START_NOT_STICKY
        }

        // The identity of the computer the person tapped, carried across the
        // service boundary. This is the whole point of the fix: the selected
        // row's *fingerprint* arrives here, so nothing downstream has to guess
        // a destination from trust-store order.
        applyRequestedTarget(intent)

        startForegroundCompat(getString(R.string.notif_connecting))

        val request = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .addTransportType(NetworkCapabilities.TRANSPORT_ETHERNET)
            .build()
        runCatching { connectivity.registerNetworkCallback(request, networkCallback) }

        // Idempotent: a second start (the user pressing Connect again, or the
        // system redelivering) must not produce a second connection loop.
        ensureCoordinator().start()

        // Not sticky: being restarted by the system without the user asking
        // is exactly the background behaviour we are avoiding.
        return START_NOT_STICKY
    }

    /**
     * Records the target named by a start intent, and re-points a live session.
     *
     * Three things have to happen together, which is why they are not spread
     * across the callers:
     *
     *  1. the choice is persisted, so it survives this service being stopped
     *     and the process being killed;
     *  2. a session running against a *different* computer is ended, because
     *     leaving it up would mean the app is connected to one peer while the
     *     person is looking at another;
     *  3. the coordinator is told, so the new target is dialled now rather
     *     than after the previous target's backoff — which for an offline
     *     first peer could be five minutes of apparent silence.
     *
     * A start intent with no target — the system redelivering, or a capability
     * screen making sure the link is up — changes nothing. It must not clear a
     * choice, and it must not re-point anything.
     */
    private fun applyRequestedTarget(intent: Intent?) {
        val hex = intent?.getStringExtra(EXTRA_TARGET_FINGERPRINT) ?: return
        val fingerprint = Fingerprint.fromHex(hex) ?: return
        // Refused for a computer that is not trusted. Selecting is not a way
        // to become trusted, and a request naming an unknown fingerprint is
        // ignored rather than allowed to clear a good choice.
        if (!app.selectPeer(fingerprint)) return
        if (currentSessionPeerHex != null && currentSessionPeerHex != hex) {
            currentConnection?.disconnect()
        }
        coordinator?.onTargetChanged(fingerprint.toDisplayShort())
    }

    @Synchronized
    private fun ensureCoordinator(): ConnectionCoordinator =
        coordinator ?: ConnectionCoordinator(
            scope = lifecycleScope,
            endpoints = ::endpointsFor,
            dial = ::dial,
            log = { event -> Log.i(TAG, event.toString()) },
            onState = ::onLinkState,
            // "Nothing is paired" and "two computers, and you have not said
            // which" are both answered by a person, not by a retry. Stating
            // them stops the loop instead of leaving a countdown on screen
            // that nothing will ever satisfy.
            blocked = { app.targetResolution().blockedReason() },
        ).also { coordinator = it }

    /**
     * Where to dial this round. Empty when there is nothing to dial.
     *
     * Re-read every round on purpose: a target chosen while a retry was
     * pending takes effect on the very next attempt, with no restart and no
     * cache to go stale.
     */
    private suspend fun endpointsFor(round: Int) =
        targetPeer()?.let { app.candidateAddresses(it, round) } ?: emptyList()

    /**
     * One dial, translated into the vocabulary the coordinator retries on.
     *
     * The distinction that matters: a transport failure is the network having
     * a bad day and is retried promptly; a security failure means the peer is
     * not the pinned identity and is retried slowly and never re-trusted; a
     * revocation is not retried at all.
     */
    private suspend fun dial(address: java.net.InetSocketAddress): DialResult {
        // Having no target is terminal rather than transient, and the reason
        // says which of the two it is. Neither "nothing is paired" nor "two
        // computers and no choice" is fixed by dialling again in two seconds,
        // and a loop that kept trying would burn battery to display a
        // countdown for something only a person can resolve.
        val resolution = app.targetResolution()
        val peer = resolution.peerOrNull()
            ?: return DialResult.Terminal(resolution.blockedReason() ?: "no paired computer")

        // UX-DEBT-02. `app.connect` carries no pairing token, by design. If a
        // scanned code is being proved for this same computer right now, a
        // tokenless connection would land on a desktop that is waiting for a
        // PAIR_REQUEST, never send one, and be declared terminal underneath a
        // pairing the person is in the middle of. Transient on purpose: the
        // loop backs off and comes straight back, so a pairing that fails
        // leaves the link where it was. See PairingGate.
        PairingGate.holdFor(peer.fingerprint.toHex(), app.pairingInFlightHex)?.let {
            return DialResult.Transient(it)
        }

        return when (val result = app.connect(peer, address)) {
            is ConnectResult.Established -> DialResult.Established {
                val connection = result.connection
                currentConnection = connection
                // Recorded before the session runs, so a retarget arriving in
                // the same instant can tell whose session this is.
                currentSessionPeerHex = peer.fingerprint.toHex()
                // What this session can actually carry, published for the UI.
                // Not a grant and never persisted: it is fixed by the HELLO
                // that just ran and dies with the session, which is exactly
                // why a screen can trust it. See PliweeApp.LiveSession.
                app.publishLiveSession(
                    PliweeApp.LiveSession(
                        peerHex = peer.fingerprint.toHex(),
                        negotiated = connection.negotiatedCapabilities.toSet(),
                        // Straight off the HELLO this session ran, never a
                        // guess and never remembered past the session.
                        platform = connection.peerDevice.platform,
                    ),
                )
                updateNotification(getString(R.string.notif_connected, peer.deviceName))
                // A data stream reuses this session's address, port and
                // pinned identity. Recorded before the session runs, so a
                // transfer offered the instant we connect has somewhere to
                // dial.
                app.files.attachTransport(
                    connection.remoteAddress,
                    peer.fingerprint,
                    connection.profile,
                )
                // Remember only an address that actually worked, so the fast
                // path stays the one that was proven, not merely advertised.
                // Established means the pinned handshake and HELLO both
                // succeeded, which is what makes this a successful hint.
                runCatching {
                    app.trustStore.recordSuccessfulAddress(peer.fingerprint, address)
                }
                try {
                    connection.run(lifecycleScope)
                } finally {
                    currentConnection = null
                    currentSessionPeerHex = null
                    // Cleared with the session it describes. A negotiated set
                    // that outlived its session would be the stale authority
                    // this flow exists to avoid.
                    app.publishLiveSession(null)
                }
            }

            // The computer forgot us. Retrying cannot help and only the user
            // can fix it, so this ends the loop rather than backing off.
            is ConnectResult.PairingRequired ->
                DialResult.Terminal("the computer no longer knows this device")

            is ConnectResult.Failed -> when (result.kind) {
                FailureKind.REVOKED -> DialResult.Terminal(result.reason)
                FailureKind.SECURITY -> DialResult.Security(result.reason)
                FailureKind.TRANSPORT -> DialResult.Transient(result.reason)
            }
        }
    }

    private fun onLinkState(state: LinkState) {
        val app = app
        when (state) {
            is LinkState.Connecting -> {
                app.publishConnectionState(PliweeApp.ConnectionState.Connecting)
                updateNotification(getString(R.string.notif_connecting))
            }

            is LinkState.Connected -> {
                val peer = targetPeer()
                app.publishConnectionState(
                    if (peer == null) {
                        PliweeApp.ConnectionState.Connecting
                    } else {
                        PliweeApp.ConnectionState.Connected(
                            peer.deviceName,
                            peer.fingerprint.toDisplayShort(),
                        )
                    },
                )
            }

            is LinkState.RetryWait -> {
                // The UI says "trying again", not "connected": a session that
                // ended must never keep looking live.
                app.publishConnectionState(
                    PliweeApp.ConnectionState.Retrying(state.reason, state.delayMs / 1000),
                )
                updateNotification(getString(R.string.notif_connecting))
            }

            is LinkState.GaveUp -> {
                app.publishConnectionState(PliweeApp.ConnectionState.Error(state.reason))
                // Nothing left to try, so holding a foreground service — and
                // its notification — would be claiming work we are not doing.
                lifecycleScope.launch { stopSelf() }
            }

            is LinkState.Stopped ->
                app.publishConnectionState(PliweeApp.ConnectionState.Idle)
        }
    }

    private val app: PliweeApp get() = application as PliweeApp

    /**
     * The computer to connect to, or null when there is no unambiguous one.
     *
     * The line this replaces — `app.trustStore.peers().firstOrNull()` — is the
     * certified U2 §39.17 defect. It answered "where to" with a list index, so
     * an Ubuntu desktop that had been powered off since the previous sprint
     * silently absorbed every connection attempt meant for a Debian desktop
     * sitting reachable on the same LAN.
     */
    private fun targetPeer(): TrustStore.TrustedPeer? =
        runCatching { app.targetPeer() }.getOrNull()

    override fun onDestroy() {
        // Being destroyed is an explicit end, not a failure: stop first so
        // nothing schedules a retry into a scope that is going away.
        coordinator?.stop("service destroyed")
        currentConnection?.disconnect()
        currentSessionPeerHex = null
        app.publishLiveSession(null)
        runCatching { connectivity.unregisterNetworkCallback(networkCallback) }
        super.onDestroy()
    }

    private fun startForegroundCompat(text: String) {
        val notification = buildNotification(text)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(NotificationManager::class.java)
        runCatching { manager.notify(NOTIFICATION_ID, buildNotification(text)) }
    }

    private fun buildNotification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .setContentIntent(open)
            .build()
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.channel_connection),
            // LOW: this is a persistent status notification, not an alert.
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    companion object {
        private const val TAG = "ConnectionService"
        private const val CHANNEL_ID = "connection"
        private const val NOTIFICATION_ID = 1

        const val ACTION_STOP = "io.github.yurisismotto.pliwee.STOP"

        /**
         * Lowercase fingerprint hex of the computer to connect to.
         *
         * A fingerprint rather than a name, an address or a list position: it
         * is the identity the TLS pin is checked against, so what the person
         * tapped and what the handshake verifies are the same value end to
         * end. A public one, so putting it in an intent leaks nothing.
         */
        const val EXTRA_TARGET_FINGERPRINT =
            "io.github.yurisismotto.pliwee.TARGET_FINGERPRINT"

        /**
         * Starts the connection, optionally re-pointing it at [target].
         *
         * [target] null means "connect to whatever is already chosen" — what a
         * capability screen wants when it just needs the link up. It never
         * means "pick one for me": with several trusted computers and no
         * choice made, the loop stops and says so instead of guessing.
         */
        fun start(context: Context, target: Fingerprint? = null) {
            val intent = Intent(context, ConnectionService::class.java)
            if (target != null) intent.putExtra(EXTRA_TARGET_FINGERPRINT, target.toHex())
            context.startForegroundService(intent)
        }

        fun stop(context: Context) {
            context.startService(
                Intent(context, ConnectionService::class.java).setAction(ACTION_STOP),
            )
        }
    }
}
