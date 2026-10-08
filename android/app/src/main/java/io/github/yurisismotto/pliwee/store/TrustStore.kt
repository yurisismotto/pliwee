package io.github.yurisismotto.pliwee.store

import android.content.Context
import io.github.yurisismotto.pliwee.capability.SensitiveCapabilities
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.notifications.NotificationSecret
import java.io.File
import java.net.InetSocketAddress
import java.security.SecureRandom
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject

/**
 * Paired computers, on disk.
 *
 * ## What is stored
 *
 * Identity metadata, pinned public-key fingerprints, per-capability grants,
 * settings. Nothing else. No message content and no transfer history.
 *
 * ## Where
 *
 * A JSON file in the app's private `filesDir`. On Android that directory is
 * already isolated per-app by the platform sandbox, and the file contains no
 * secrets: the private key lives in the Keystore and never touches this file,
 * and a fingerprint is a public value. Encrypting it would add a key
 * management problem without protecting anything that is not already public.
 *
 * The `schemaVersion` field exists from the first commit so later migrations
 * are possible.
 *
 * ## Observability
 *
 * [peersFlow] is the single source of truth for the UI. Reading [peers] once
 * during composition is what made the previous release's device card ignore a
 * grant that changed elsewhere: the value was correct when read and never
 * read again. Every mutator below publishes, so a screen that collects the
 * flow cannot show stale trust state.
 */
class TrustStore(context: Context) : NotificationSecret.Metadata {

    private val file = File(context.filesDir, FILE_NAME)
    private var state: JSONObject = load()

    /**
     * Every stored record, revoked and tombstoned ones included.
     *
     * The truth the two published views are derived from. Only
     * [peerRecord] and the mutators read it: anything that decides whether
     * something may happen asks [peers] or [peer], which exclude the revoked.
     */
    private var records: List<TrustedPeer> = readPeers()

    private val _peersFlow = MutableStateFlow(trustedOf(records))

    /**
     * Every **trusted** computer, republished on every change.
     *
     * The UI must observe this rather than calling [peers], so that a grant
     * or a clipboard policy changed on one screen is visible on every other.
     *
     * Revoked records are excluded. A screen that wants to show them — the
     * device list, which is where they are removed from — collects
     * [listedPeersFlow] instead, and is the only thing that does.
     */
    val peersFlow: StateFlow<List<TrustedPeer>> = _peersFlow.asStateFlow()

    private val _listedPeersFlow = MutableStateFlow(listedOf(records))

    /**
     * What belongs in the device list: trusted computers, and revoked ones
     * the person has not yet removed.
     *
     * Deliberately a second flow rather than a flag the screen filters on. A
     * screen that filtered would be one forgotten `filter` away from offering
     * a Connect button for a revoked computer; a screen that cannot see the
     * trusted set and the listed set as the same thing cannot make that
     * mistake by omission.
     */
    val listedPeersFlow: StateFlow<List<TrustedPeer>> = _listedPeersFlow.asStateFlow()

    private val _selectedPeerFlow = MutableStateFlow(readSelectedPeer())

    /**
     * The computer the person chose to connect to, as a fingerprint hex.
     *
     * A *hex string* rather than a [Fingerprint] on purpose: `Fingerprint`
     * wraps a `ByteArray`, whose `equals` is identity, so a `StateFlow` of one
     * would republish on every re-read and compare wrongly. Hex is also the
     * spelling the UI already navigates by.
     *
     * Null means nobody has chosen — which is a real state, not a broken one.
     * [PeerTarget] decides what that means; this only stores it.
     */
    val selectedPeerFlow: StateFlow<String?> = _selectedPeerFlow.asStateFlow()

    val deviceId: String get() = state.getString(KEY_DEVICE_ID)

    var deviceName: String
        get() = state.optString(KEY_DEVICE_NAME, android.os.Build.MODEL ?: "Android")
        set(value) {
            state.put(KEY_DEVICE_NAME, sanitizeDeviceName(value))
            persist()
        }

    data class TrustedPeer(
        val deviceId: String,
        val deviceName: String,
        val fingerprint: Fingerprint,
        val pairedAtUnix: Long,
        val grantedCapabilities: Set<String>,
        /**
         * Where an authenticated session with this computer last came up,
         * most recent first, at most [AddressHint.MAX_PER_PEER].
         *
         * Routing hints, never identity: the pinned [fingerprint] decides who
         * answered whichever hint is dialled. See [AddressHint].
         */
        val addressHints: List<AddressHint>,
        /**
         * Whether the person withdrew trust from this computer.
         *
         * Before this existed the only thing the phone could do was *delete*
         * the record, and a deleted record is not a revocation: the same key
         * became a computer this phone had never met, free to be paired again
         * with nothing to say it had ever been thrown out, and with no trace
         * of the decision for anyone to see. Revoking keeps the pinned
         * identity and stops it being a destination.
         *
         * A revoked record is **not** a peer. [TrustStore.peers] and
         * [TrustStore.peer] both exclude it, which is what makes every
         * existing caller — the connection coordinator, [PeerTarget], the
         * grant and policy lookups, the share sheet — fail closed without
         * being changed.
         */
        val revoked: Boolean = false,
        /**
         * Whether the person also took the revoked record off the list — the
         * *tombstone* half.
         *
         * Presentation, never admission. Hidden implies revoked, and
         * [fromJson] decides that rather than believing the file: a record
         * claiming to be out of sight and still trusted is the one
         * combination that would be dangerous to read literally.
         */
        val hidden: Boolean = false,
        /**
         * Per-peer `clipboard.v1` direction and automation settings.
         *
         * Stored next to the grant but deliberately separate from it: the
         * grant says whether this computer may speak clipboard at all, this
         * says in which directions and how automatically. Both are decided
         * locally — no protocol message writes either — and neither holds
         * clipboard content.
         */
        val clipboardPolicy: ClipboardPolicy = ClipboardPolicy(),
        /**
         * Per-peer `notifications.v1` mirroring settings.
         *
         * Beside the grant and deliberately separate from it, exactly as
         * [clipboardPolicy] is: the grant says whether this computer may
         * receive notifications at all, this says which ones and how much of
         * each. Both are decided locally — **no protocol message writes
         * either** — and neither holds a notification's content. The app list
         * is empty by default, so a freshly granted computer receives nothing
         * until a person names an application.
         */
        val notificationPolicy: NotificationPolicy = NotificationPolicy(),
    ) {
        fun allows(capabilityId: String): Boolean =
            !revoked && capabilityId in grantedCapabilities

        /** Whether this record belongs in an ordinary device list. */
        fun isListed(): Boolean = !hidden

        /**
         * The remembered hints as dialable addresses, most recent first.
         *
         * Empty for a revoked record whatever it holds: a revoked computer is
         * not a destination, and a hint left on one must not make it one.
         */
        fun dialAddresses(): List<InetSocketAddress> =
            if (revoked) emptyList() else addressHints.map { it.toSocketAddress() }

        /**
         * The record after an authenticated session came up at [success].
         *
         * Unchanged for a revoked record: nothing is remembered about where
         * to dial a computer this phone has decided not to dial.
         */
        fun withSuccessfulAddress(success: AddressHint): TrustedPeer =
            if (revoked) this else copy(addressHints = AddressHint.recordSuccess(addressHints, success))

        /**
         * The record with trust withdrawn.
         *
         * The grants and both policies go with it. They are inert while the
         * grant is gone — every authorizer asks both — but leaving
         * `allowMirror` or a twenty-application list behind on a record that
         * can be paired again is a decision waiting to be resurrected, and
         * this is the moment the person said no. The remembered addresses go
         * too: they are a convenience for dialling something this phone has
         * just decided not to dial.
         *
         * `DENIED` rather than a fresh default. The two are equivalent to
         * every caller — nothing reads a policy without asking about the
         * grant — but a *default* policy serializes as `allowMirror: true`,
         * and `NotificationPolicy()`'s `knownApps` is the list of every
         * application installed on this phone, cached for the picker. Neither
         * belongs on the record of a computer the person just said no to, and
         * the second is the one that matters: it is a few hundred package
         * names, which is a fingerprint of the person's life.
         */
        fun asRevoked(): TrustedPeer = copy(
            revoked = true,
            hidden = false,
            grantedCapabilities = emptySet(),
            addressHints = emptyList(),
            clipboardPolicy = ClipboardPolicy.DENIED,
            notificationPolicy = NotificationPolicy.DENIED,
        )

        /**
         * The record reduced to the revocation itself.
         *
         * What survives is what the rules read: the pinned fingerprint, which
         * *is* the identity, and `revoked`. The name, the device id, the
         * pairing time, the addresses, the grants and the policies are facts
         * about a relationship that has ended, and none of them is needed to
         * keep refusing the key. Dropping them is both the privacy answer and
         * the security one — a grant that is not stored cannot come back on a
         * re-pair.
         */
        fun asTombstone(): TrustedPeer = TrustedPeer(
            deviceId = "",
            deviceName = "",
            fingerprint = fingerprint,
            pairedAtUnix = 0,
            grantedCapabilities = emptySet(),
            addressHints = emptyList(),
            revoked = true,
            hidden = true,
            clipboardPolicy = ClipboardPolicy.DENIED,
            notificationPolicy = NotificationPolicy.DENIED,
        )

        /**
         * One record, as it is written.
         *
         * Pure and beside [fromJson], so the two halves of the format can be
         * read together and round-tripped in a JVM test. Nothing here can
         * hold clipboard text, a notification's content, a pairing token or a
         * proof — there is no field on this type that could.
         */
        fun toJson(): JSONObject = JSONObject().apply {
            put(KEY_DEVICE_ID, deviceId)
            put(KEY_DEVICE_NAME, sanitizeDeviceName(deviceName))
            put(KEY_FINGERPRINT, fingerprint.toHex())
            put(KEY_PAIRED_AT, pairedAtUnix)
            put(KEY_GRANTS, JSONArray(grantedCapabilities.toList()))
            // The old `host:port` strings, still written, so an older build
            // reading this file dials the same places it always did. They are
            // derived from the hints and never read while the hints are there.
            put(KEY_ADDRESSES, JSONArray(addressHints.map { it.format() }))
            // Written only when there are any, for the reason the flags
            // below are: a record with nothing to remember is what an older
            // build wrote.
            if (addressHints.isNotEmpty()) {
                put(KEY_ADDRESS_HINTS, JSONArray(addressHints.map { it.toJson() }))
            }
            // Written only when true, so a file full of ordinary trusted
            // computers is byte-for-byte what an older build wrote and an
            // older build can still read it.
            if (revoked) put(KEY_REVOKED, true)
            if (hidden) put(KEY_HIDDEN, true)
            // Settings, never content: the policy flags are stored, and no
            // clipboard text ever reaches this file.
            put(KEY_CLIPBOARD_POLICY, clipboardPolicy.toJson())
            // Settings, never content: the flags and the list of package
            // names the person chose. No notification title, body, subtext or
            // platform key reaches this file, and there is no field here that
            // could hold one.
            put(KEY_NOTIFICATION_POLICY, notificationPolicy.toJson())
        }

        companion object {
            /**
             * One record, as it is read — or null when it is not a record.
             *
             * A missing or malformed fingerprint is the one unrecoverable
             * case: a record with no pinned key is not an identity, and
             * guessing one would be worse than dropping it.
             *
             * Absent flags read false, which is the migration: a file written
             * before this sprint has neither key, and every record in it is a
             * trusted, visible one — which is exactly what it was. Nothing is
             * hidden for anybody on upgrade.
             */
            fun fromJson(entry: JSONObject): TrustedPeer? {
                val fingerprint = Fingerprint.fromHex(entry.optString(KEY_FINGERPRINT))
                    ?: return null
                val hidden = entry.optBoolean(KEY_HIDDEN, false)
                // Hidden implies revoked, decided here rather than believed
                // from the file. A record that claims to be off the list and
                // still trusted is either hand-edited or damaged, and
                // believing it would mean a computer nobody can see keeping
                // its grants.
                val revoked = hidden || entry.optBoolean(KEY_REVOKED, false)
                return TrustedPeer(
                    deviceId = entry.optString(KEY_DEVICE_ID),
                    deviceName = entry.optString(KEY_DEVICE_NAME),
                    fingerprint = fingerprint,
                    pairedAtUnix = entry.optLong(KEY_PAIRED_AT),
                    grantedCapabilities = entry.optJSONArray(KEY_GRANTS)
                        ?.let { grants -> (0 until grants.length()).map { grants.getString(it) } }
                        ?.toSet()
                        ?: emptySet(),
                    // A revoked record keeps no usable hint, whatever the file
                    // says: revoking clears them, and one that survived on
                    // disk is a destination this phone decided not to dial.
                    addressHints = if (revoked) emptyList() else addressHintsOf(entry),
                    revoked = revoked,
                    hidden = hidden,
                    // A record written before this capability existed has no
                    // policy object; `fromJson` supplies the documented
                    // defaults rather than turning everything off — or, worse,
                    // on.
                    clipboardPolicy = ClipboardPolicy.fromJson(
                        entry.optJSONObject(KEY_CLIPBOARD_POLICY),
                    ),
                    notificationPolicy = NotificationPolicy.fromJson(
                        entry.optJSONObject(KEY_NOTIFICATION_POLICY),
                    ),
                )
            }

            /**
             * A record's address hints — the migration from `addresses`.
             *
             * `addressHints` when the record has them. Otherwise the old
             * `addresses` strings, which are what every build before this
             * one wrote: each becomes a hint, in the order stored — most
             * recent first, as the old merge kept it — with its last success
             * unknown. Either way the result is normalized, de-duplicated
             * and bounded, so an over-long or hand-edited list is reduced the
             * same way every time. An entry that cannot name an endpoint is
             * dropped: hints are non-authoritative, and losing one costs a
             * discovery round, never a pairing.
             *
             * Nothing else in the record depends on this. The fingerprint,
             * the device id, the grants and the policies are read exactly as
             * before, whatever the hints turn out to be.
             */
            fun addressHintsOf(entry: JSONObject): List<AddressHint> {
                val structured = entry.optJSONArray(KEY_ADDRESS_HINTS)
                val hints = if (structured != null) {
                    (0 until structured.length()).mapNotNull { index ->
                        structured.optJSONObject(index)?.let(AddressHint::fromJson)
                    }
                } else {
                    val legacy = entry.optJSONArray(KEY_ADDRESSES) ?: return emptyList()
                    (0 until legacy.length()).mapNotNull { index ->
                        AddressHint.parseLegacy(legacy.optString(index))
                    }
                }
                return AddressHint.bounded(hints)
            }

            /**
             * What a record becomes when its computer is paired again.
             *
             * Pure, and separate from the store, so the rule can be read and
             * tested without a device. [TrustStore.upsertPairedPeer] is its
             * only caller and the reason it exists is written there.
             *
             *  * `existing == null`, or a record for a different key —
             *    nothing is known about this fingerprint, so [paired] is the
             *    record. Normal new-peer semantics, whatever the QR was
             *    scanned over, and nothing is inherited from another key.
             *  * otherwise — facts refresh, decisions persist.
             *
             * And in every branch: **a pairing never grants a sensitive
             * capability.** Preserving a grant the person made is the point;
             * inventing one because the fresh negotiation advertised it is
             * the thing this must not do. See [SensitiveCapabilities].
             */
            fun mergePairing(
                existing: TrustedPeer?,
                paired: TrustedPeer,
            ): TrustedPeer {
                // The non-escalation boundary, applied once and used by both
                // branches: a pairing may never be the thing that grants a
                // sensitive capability. Negotiating one only means both sides
                // *support* it. See [SensitiveCapabilities] for why this is
                // enforced here rather than left to the caller — it used to
                // be, in a different file, and that made a real property an
                // accident of where a line happened to live.
                val grantable =
                    paired.grantedCapabilities - SensitiveCapabilities.NEVER_AUTO_GRANTED

                // Nothing is known about this key, or the record offered is a
                // *different* key's. Either way this is a new peer and there
                // is nothing to inherit: grants, policies and pairing time all
                // come from the pairing, and never from another fingerprint's
                // record. The fingerprint check is belt and braces on top of
                // the caller looking the record up by fingerprint, because
                // inheriting another computer's notification grant is the one
                // mistake this function must be structurally unable to make.
                //
                // `existing.revoked` joins that list, and it is the reason
                // this sprint touched a function it otherwise would not have.
                // A revoked record is a person who said "no longer" about
                // *this* key; pairing it again is a fresh yes, and a fresh yes
                // must not quietly restore the grants, the clipboard
                // directions or the notification app list that the "no
                // longer" took away. Without this line a tombstone would be a
                // *better* place for a stale grant to hide than a deleted
                // record was — invisible on screen and still inherited.
                //
                // It does not affect the case this merge was written for. A
                // desktop-side revoke leaves the phone's own record untouched
                // (`revoked` is false), so UX-DEBT-02 still holds: the
                // person's local decisions survive somebody else's revoke and
                // only their own erases them.
                if (existing == null ||
                    existing.revoked ||
                    !existing.fingerprint.contentEquals(paired.fingerprint)
                ) {
                    return paired.copy(
                        grantedCapabilities = grantable,
                        addressHints = AddressHint.bounded(paired.addressHints),
                        // A pairing that succeeded is a visible, trusted row,
                        // whatever was in its place a moment ago.
                        revoked = false,
                        hidden = false,
                    )
                }

                return existing.copy(
                    revoked = false,
                    hidden = false,
                    // Facts about the session that just authenticated.
                    deviceId = paired.deviceId,
                    deviceName = paired.deviceName,
                    pairedAtUnix = paired.pairedAtUnix,
                    // The address that answered first, then what was already
                    // remembered. Never fewer places to look than before, up
                    // to the bound; the same endpoint twice is one hint, with
                    // the pairing's fresher time.
                    addressHints = AddressHint.bounded(
                        paired.addressHints + existing.addressHints,
                    ),
                    // Decisions the person made about this fingerprint.
                    //
                    // Unioned, so an auto-grantable capability this pairing
                    // negotiated is added and one they had already allowed is
                    // not removed. A *sensitive* capability is in this result
                    // if and only if `existing` already held it — which is to
                    // say, if and only if the person granted it themselves.
                    grantedCapabilities = existing.grantedCapabilities + grantable,
                    // clipboardPolicy and notificationPolicy are `existing`'s
                    // by virtue of `copy`, and that is the point: they are
                    // settings, not facts about the handshake.
                )
            }
        }
    }

    /**
     * The computers this phone trusts.
     *
     * Revoked records are **not** here, and that is the whole reason the
     * revoked flag could be added without touching the connection
     * coordinator, [PeerTarget], the share sheet, the clipboard tile or any
     * capability lookup: every one of them asks this question and every one of
     * them now fails closed on a revoked computer for free.
     */
    fun peers(): List<TrustedPeer> = _peersFlow.value

    /** Every stored record, revoked and tombstoned ones included. */
    fun allRecords(): List<TrustedPeer> = records

    private fun readPeers(): List<TrustedPeer> {
        val array = state.optJSONArray(KEY_PEERS) ?: return emptyList()
        return (0 until array.length()).mapNotNull { index ->
            array.optJSONObject(index)?.let { TrustedPeer.fromJson(it) }
        }
    }

    /**
     * One trusted computer, by pinned fingerprint.
     *
     * Revoked records are excluded, so the grant and policy lookups built on
     * this return their DENIED defaults for one without any of them having to
     * remember to ask a second question.
     */
    fun peer(fingerprint: Fingerprint): TrustedPeer? =
        peers().firstOrNull { it.fingerprint.contentEquals(fingerprint) }

    /** One record of any kind, revoked and tombstoned included. */
    fun peerRecord(fingerprint: Fingerprint): TrustedPeer? =
        records.firstOrNull { it.fingerprint.contentEquals(fingerprint) }

    fun addPeer(peer: TrustedPeer) {
        writePeers(upserted(records, peer))
    }

    /**
     * Records a pairing that has just been proved, keeping what the person
     * already decided about this computer.
     *
     * ## UX-DEBT-02
     *
     * [addPeer] replaces a record wholesale, which is right for a computer
     * being met for the first time and wrong for one being paired *again*.
     * A desktop that revoked this phone and was then re-paired from a fresh
     * QR came back as a brand-new row: the clipboard grant the person had
     * turned on was off, the notification app list they had chosen was empty,
     * and nothing said so. Re-pairing is not a reason to undo somebody's
     * settings.
     *
     * The rule, stated once here rather than at the call site:
     *
     *  * **facts refresh.** Device id, display name, the address that just
     *    answered and the pairing time come from the session that has this
     *    moment authenticated, because that is what they are *about*;
     *  * **decisions persist.** Capability grants, the clipboard policy and
     *    the notification policy are the phone owner's, about this
     *    fingerprint, and a re-pair of the same fingerprint does not revisit
     *    them. Grants are unioned, so a capability this pairing negotiated is
     *    added and one the person had already allowed is not taken away.
     *
     * Safe because the fingerprint is the anchor: it is the pinned SPKI the
     * TLS handshake was checked against and the identity the proof was bound
     * to, so "the same fingerprint" means the same key, not the same name and
     * not the same address. A *different* fingerprint matches nothing here
     * and takes the ordinary new-peer path through [addPeer].
     *
     * Called only after a proof succeeded or the desktop declared this device
     * already trusted. Nothing in this class is what decides that.
     */
    fun upsertPairedPeer(peer: TrustedPeer): TrustedPeer {
        // Looked up by fingerprint and by nothing else: the pinned SPKI is
        // the anchor, so a changed device id, display name or address updates
        // a record while a changed key is simply a different computer.
        val merged = TrustedPeer.mergePairing(
            existing = peer(peer.fingerprint),
            paired = peer,
        )
        addPeer(merged)
        return merged
    }

    /**
     * Withdraws trust from a computer, keeping the record.
     *
     * This replaced `removePeer`, which deleted. The deletion was the defect:
     * a deleted key is a computer this phone has never met, so nothing was
     * left to say the person had thrown it out, nothing stopped it being
     * paired again as an ordinary stranger, and the row simply vanished with
     * no account of why. Now the row stays, says **Revoked**, and the person
     * decides separately whether to take it off the list.
     *
     * Returns false when there is no such record. Revoking something already
     * revoked is a no-op rather than a second write.
     */
    fun revokePeer(fingerprint: Fingerprint): Boolean {
        val existing = peerRecord(fingerprint) ?: return false
        if (existing.revoked) return true
        // The choice goes with the trust. Leaving the hex behind would be a
        // dangling instruction to dial something that is no longer trusted —
        // harmless, because `PeerTarget.resolve` ignores an unmatched choice,
        // but it would silently re-target if the same key were ever paired
        // again, which is not a decision this method is entitled to make.
        applySelectionAfterRemoval(fingerprint)
        addPeer(existing.asRevoked())
        return true
    }

    /**
     * Takes one **already revoked** computer off the list, keeping the
     * revocation.
     *
     * The record is replaced by [TrustedPeer.asTombstone] — the pinned
     * fingerprint and `revoked`, nothing else. It is deliberately not a
     * deletion: deleting would make the same key unknown, and an unknown key
     * is one this phone would be willing to pair with as though nothing had
     * happened, inheriting nothing but also warning nobody.
     *
     * Refuses a computer that is still trusted, and says so by returning
     * false. Taking a row off a list must never be a way to withdraw trust
     * without saying so — [revokePeer] is the deliberate act and it comes
     * first.
     *
     * Purely local: nothing is sent, no session is needed, and there is no
     * network dependency of any kind.
     */
    fun hideRevokedPeer(fingerprint: Fingerprint): Boolean {
        val existing = peerRecord(fingerprint) ?: return false
        if (!existing.revoked) return false
        if (existing.hidden) return true
        applySelectionAfterRemoval(fingerprint)
        addPeer(existing.asTombstone())
        return true
    }

    /** Applies [selectionAfterRemoval] to the stored choice. */
    private fun applySelectionAfterRemoval(removed: Fingerprint) {
        if (selectionAfterRemoval(selectedPeerHex, removed.toHex()) == null) {
            clearSelectedPeer()
        }
    }

    /** The chosen computer's fingerprint hex, or null when nobody has chosen. */
    val selectedPeerHex: String? get() = _selectedPeerFlow.value

    /**
     * Records which computer the person chose to connect to.
     *
     * Refuses a fingerprint that is not trusted, and says so by returning
     * false: choosing a destination is not a way to become trusted, and a
     * discovered-but-unpaired computer must still go through pairing. Callers
     * treat false as "nothing was selected", never as "selected anyway".
     */
    fun selectPeer(fingerprint: Fingerprint): Boolean {
        if (peer(fingerprint) == null) return false
        val hex = fingerprint.toHex()
        if (_selectedPeerFlow.value == hex) return true
        state.put(KEY_SELECTED_PEER, hex)
        persist()
        _selectedPeerFlow.value = hex
        return true
    }

    /** Forgets the choice, without forgetting any computer. */
    fun clearSelectedPeer() {
        if (_selectedPeerFlow.value == null) return
        state.remove(KEY_SELECTED_PEER)
        persist()
        _selectedPeerFlow.value = null
    }

    private fun readSelectedPeer(): String? {
        val hex = state.optString(KEY_SELECTED_PEER).takeIf { it.isNotEmpty() } ?: return null
        // Validated rather than trusted: a hand-edited or truncated value must
        // not become a target, and `fromHex` is the one strict spelling.
        return if (Fingerprint.fromHex(hex) != null) hex else null
    }

    /**
     * Grants or withdraws one capability for one computer.
     *
     * Takes effect immediately: the grant is re-read from here on every
     * question, so withdrawing it stops traffic on a session that is already
     * connected rather than at the next reconnect.
     */
    fun setGrant(fingerprint: Fingerprint, capabilityId: String, granted: Boolean) {
        val peer = peer(fingerprint) ?: return
        val grants = peer.grantedCapabilities.toMutableSet()
        if (granted) grants += capabilityId else grants -= capabilityId
        addPeer(peer.copy(grantedCapabilities = grants))
    }

    /** Replaces one computer's clipboard policy. */
    fun setClipboardPolicy(fingerprint: Fingerprint, policy: ClipboardPolicy) {
        val peer = peer(fingerprint) ?: return
        addPeer(peer.copy(clipboardPolicy = policy))
    }

    /**
     * The effective clipboard policy for a computer, right now.
     *
     * One call answers the grant *and* the policy on purpose. A caller that
     * had to ask both separately could do the second and forget the first,
     * and the failure would be silent — a forgotten computer whose stored
     * policy still said `allowReceive`.
     */
    fun clipboardPolicyFor(fingerprint: Fingerprint): ClipboardPolicy {
        val peer = peer(fingerprint) ?: return ClipboardPolicy.DENIED
        if (!peer.allows(CLIPBOARD_CAPABILITY_ID)) return ClipboardPolicy.DENIED
        return peer.clipboardPolicy
    }

    /** Replaces one computer's notification policy. */
    fun setNotificationPolicy(fingerprint: Fingerprint, policy: NotificationPolicy) {
        val peer = peer(fingerprint) ?: return
        addPeer(peer.copy(notificationPolicy = policy))
    }

    /**
     * The effective notification policy for a computer, right now.
     *
     * One call answers the grant *and* the policy, for the reason
     * [clipboardPolicyFor] does: a caller that had to ask both separately
     * could do the second and forget the first, and the failure would be
     * silent — a forgotten computer whose stored policy still named twenty
     * applications.
     */
    fun notificationPolicyFor(fingerprint: Fingerprint): NotificationPolicy {
        val peer = peer(fingerprint) ?: return NotificationPolicy.DENIED
        if (!peer.allows(NOTIFICATIONS_CAPABILITY_ID)) return NotificationPolicy.DENIED
        return peer.notificationPolicy
    }

    /**
     * How many times a `device_notification_secret` has been created here.
     *
     * A counter, and nothing else. It is not the secret, it is not derived
     * from it, and it reveals nothing: its only job is to let
     * [NotificationSecret.loadOrCreate] tell "this install has never had a
     * secret" apart from "this install had one and it is gone", so that a
     * regeneration is reported rather than silent. See ADR-0016 §5.
     */
    override var notificationSecretGeneration: Int
        get() = state.optInt(KEY_NOTIFICATION_SECRET_GENERATION, 0)
        set(value) {
            state.put(KEY_NOTIFICATION_SECRET_GENERATION, value)
            persist()
        }

    /**
     * Remembers where a peer was last reachable, to skip discovery next time.
     *
     * Call only once a session with the pinned [fingerprint] is established:
     * that is what makes [address] a *successful* hint rather than merely an
     * advertised one. The hint moves to the front with the current time, an
     * equivalent older one collapses into it, and the list stays bounded.
     *
     * Does nothing for a computer that is not trusted, revoked included.
     */
    fun recordSuccessfulAddress(fingerprint: Fingerprint, address: InetSocketAddress) {
        val peer = peer(fingerprint) ?: return
        val hint = AddressHint.from(address, System.currentTimeMillis() / 1000) ?: return
        addPeer(peer.withSuccessfulAddress(hint))
    }

    private fun writePeers(peers: List<TrustedPeer>) {
        val array = JSONArray()
        for (peer in peers) {
            array.put(peer.toJson())
        }
        state.put(KEY_PEERS, array)
        persist()
        // Re-read rather than assumed, so what is published is what the file
        // now says — including the hidden-implies-revoked correction, which
        // happens on the way in.
        records = readPeers()
        // Published after the write, so an observer that reacts by reading
        // the file sees what was published.
        _peersFlow.value = trustedOf(records)
        _listedPeersFlow.value = listedOf(records)
    }

    private fun load(): JSONObject {
        val existing = runCatching { JSONObject(file.readText()) }.getOrNull()
        if (existing != null && existing.optInt(KEY_SCHEMA) == SCHEMA_VERSION) {
            return existing
        }
        if (existing != null && existing.optInt(KEY_SCHEMA) > SCHEMA_VERSION) {
            // Refuse to reinterpret a newer file with older rules: silently
            // dropping fields we do not understand could drop a revocation.
            error("trust store schema is newer than this app supports")
        }
        return JSONObject().apply {
            put(KEY_SCHEMA, SCHEMA_VERSION)
            put(KEY_DEVICE_ID, randomDeviceId())
            put(KEY_DEVICE_NAME, android.os.Build.MODEL ?: "Android")
            put(KEY_PEERS, JSONArray())
        }.also { file.writeText(it.toString()) }
    }

    private fun persist() {
        // Write to a temp file and rename, so an interrupted write cannot
        // leave a truncated trust store behind.
        val temp = File(file.parentFile, "$FILE_NAME.tmp")
        temp.writeText(state.toString())
        temp.renameTo(file)
    }

    companion object {
        private const val FILE_NAME = "trust-store.json"
        const val SCHEMA_VERSION = 1

        private const val KEY_SCHEMA = "schemaVersion"
        private const val KEY_DEVICE_ID = "deviceId"
        private const val KEY_DEVICE_NAME = "deviceName"
        private const val KEY_PEERS = "peers"
        private const val KEY_FINGERPRINT = "fingerprint"
        private const val KEY_PAIRED_AT = "pairedAtUnix"
        private const val KEY_GRANTS = "grantedCapabilities"
        private const val KEY_ADDRESSES = "addresses"

        /**
         * Structured address hints (Mesh V2 SPEC §§5, 11).
         *
         * Additive and optional, so [SCHEMA_VERSION] stays at 1 for the
         * reason [KEY_SELECTED_PEER] did. A record without it is read from
         * [KEY_ADDRESSES], which is the migration; a record with it is read
         * from it alone. [KEY_ADDRESSES] is still written beside it, derived
         * from the hints, so an older build reading the file dials the same
         * places — and, rewriting the record, simply drops the hints, which
         * the next upgrade migrates again from the strings it left.
         */
        private const val KEY_ADDRESS_HINTS = "addressHints"

        /**
         * Trust withdrawn, and taken off the list.
         *
         * Both additive and both optional, so [SCHEMA_VERSION] stays at 1 for
         * the reason [KEY_SELECTED_PEER] did: a file written before they
         * existed reads as "trusted and visible", which is exactly what those
         * records were, and an older build reading a file that has them
         * ignores keys it does not know rather than refusing the file. There
         * is nothing to migrate in either direction.
         *
         * The one asymmetry worth naming: an older build reading a *revoked*
         * record would read it as trusted. That is not a new exposure — the
         * older build is the one that had no revoked state at all, and it can
         * only appear by downgrading the app over its own data — and it is
         * why the flags are written only when true, so the file an older build
         * sees is unchanged for every record it could have written itself.
         */
        private const val KEY_REVOKED = "revoked"
        private const val KEY_HIDDEN = "hiddenFromUi"
        private const val KEY_CLIPBOARD_POLICY = "clipboardPolicy"
        private const val KEY_NOTIFICATION_POLICY = "notificationPolicy"
        private const val KEY_NOTIFICATION_SECRET_GENERATION = "notificationSecretGeneration"

        /**
         * The chosen connection target, as a fingerprint hex.
         *
         * Additive and optional, so [SCHEMA_VERSION] stays at 1: a file
         * written before this key existed reads as "nobody has chosen", which
         * is exactly right, and an older build reading a file that has it
         * ignores a key it does not know rather than refusing the file. There
         * is nothing to migrate in either direction.
         */
        private const val KEY_SELECTED_PEER = "selectedPeer"

        /** Duplicated from `ClipboardCapability.ID` to avoid a cycle. */
        const val CLIPBOARD_CAPABILITY_ID = "clipboard.v1"

        /** Duplicated from `NotificationsCapability.ID`, for the same reason. */
        const val NOTIFICATIONS_CAPABILITY_ID = "notifications.v1"

        /**
         * The record list with [peer] replacing whatever shared its key.
         *
         * Keyed on the fingerprint and on nothing else, which is what makes
         * one cryptographic identity one row: two computers with the same
         * name, the same device id and the same address are two rows, and the
         * same key paired twice is one.
         *
         * Pure and here rather than inline in [addPeer] so the property can be
         * tested without a `Context`.
         */
        fun upserted(
            records: List<TrustedPeer>,
            peer: TrustedPeer,
        ): List<TrustedPeer> =
            records.filterNot { it.fingerprint.contentEquals(peer.fingerprint) } + peer

        /** The computers that may be connected to and granted things. */
        fun trustedOf(records: List<TrustedPeer>): List<TrustedPeer> =
            records.filterNot { it.revoked }

        /** The rows a device list shows: trusted, plus revoked-but-not-removed. */
        fun listedOf(records: List<TrustedPeer>): List<TrustedPeer> =
            records.filter { it.isListed() }

        /**
         * What the chosen computer becomes when one is revoked or removed.
         *
         * The whole rule is the comparison. An unrelated choice survives, and
         * **nothing is chosen in its place** — not the next computer, not the
         * only one left, not one with the same name. Picking a replacement is
         * the guess [PeerTarget] exists to refuse, and a removal is not a
         * special case that earns one.
         *
         * Matching is on the fingerprint hex, the pinned identity, because
         * that is the only thing that identifies a computer here.
         */
        fun selectionAfterRemoval(selectedHex: String?, removedHex: String): String? =
            if (selectedHex == removedHex) null else selectedHex

        /** 128 random bits, hex. Not derived from any hardware identifier. */
        fun randomDeviceId(): String {
            val bytes = ByteArray(16)
            SecureRandom().nextBytes(bytes)
            return bytes.joinToString("") { "%02x".format(it) }
        }

        /**
         * A device name arrives from the network and is shown in the UI.
         * Control characters could forge notification text, so they go.
         */
        fun sanitizeDeviceName(name: String): String =
            name.filter { !it.isISOControl() }.take(64)
    }
}
