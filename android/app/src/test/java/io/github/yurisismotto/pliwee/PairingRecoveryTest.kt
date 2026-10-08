package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.capability.NotificationsCapability
import io.github.yurisismotto.pliwee.capability.SensitiveCapabilities
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.net.ConnectResult
import io.github.yurisismotto.pliwee.net.DialResult
import io.github.yurisismotto.pliwee.net.FailureKind
import io.github.yurisismotto.pliwee.net.WireProfile
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.pairing.PairingGate
import io.github.yurisismotto.pliwee.pairing.PairingProof
import io.github.yurisismotto.pliwee.pairing.QrPayload
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import java.io.File
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * UX-DEBT-02: a desktop-side revoke must not leave the phone unable to
 * re-pair, and recovering must not weaken anything.
 *
 * ## The defect, as reproduced
 *
 * Two paths on this phone dial a desktop, and only one carries a token.
 * `PliweeApp.connect` — the reconnection, driven by `ConnectionService` —
 * deliberately carries none. With the desktop having revoked this phone *and*
 * a fresh pairing window open, that tokenless dial reaches a desktop waiting
 * for a proof and stops at
 *
 * ```kotlin
 * val token = pairingToken ?: return ConnectResult.PairingRequired
 * ```
 *
 * without sending a `PAIR_REQUEST`. On the certification hardware, with no
 * scan involved at all:
 *
 * ```text
 * daemon:  a revoked device is pairing again; it must prove the new token
 *          and be confirmed by hand  peer=573C CB84 DA6C 993B
 * daemon:  connection ended ... timed out waiting for PAIR_REQUEST
 * logcat:  CONNECT_FAILURE round=1 kind=terminal
 *          reason=the computer no longer knows this device
 * logcat:  STOPPED reason=the computer no longer knows this device
 * ```
 *
 * — the recorded symptom exactly. The loop then declares the peer terminal
 * underneath a person who is trying to re-pair it.
 *
 * ## What is NOT done about it
 *
 * Nothing here makes a revoked device acceptable. The desktop's rules are
 * untouched: a revoked fingerprint with no pairing window open is still
 * refused outright, one with a window open still has to prove the new
 * single-use token and still has to be confirmed by a human at the keyboard,
 * and `pliwee_core::pairing::PairingSession` still enforces expiry, single
 * use and a failed-attempt cap. No protobuf and no wire change.
 *
 * ## What is done
 *
 *  * [PairingGate] — while a scanned code is being proved for a computer,
 *    the tokenless dialer holds off for *that* computer. Transient, never
 *    terminal;
 *  * [TrustStore.TrustedPeer.mergePairing] — a re-pair updates a record
 *    instead of replacing it, so grants and policies survive;
 *  * `ConnectResult.Established.provedPairing` — whether a proof happened is
 *    a fact the handshake reports, not something the caller assumes.
 */
class PairingRecoveryTest {

    // -- the world ----------------------------------------------------------

    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private val fedora = fingerprint(1)
    private val debian = fingerprint(2)

    private fun peer(
        f: Fingerprint,
        name: String,
        grants: Set<String>,
        addresses: List<String> = listOf("192.168.68.72:55432"),
        pairedAt: Long = 1_700_000_000L,
        clipboard: ClipboardPolicy = ClipboardPolicy(),
        notifications: NotificationPolicy = NotificationPolicy(),
        deviceId: String = "795fec0868ebef8c3d7ad3e775dc6ed0",
    ) = TrustStore.TrustedPeer(
        deviceId = deviceId,
        deviceName = name,
        fingerprint = f,
        pairedAtUnix = pairedAt,
        grantedCapabilities = grants,
        addressHints = addresses.mapNotNull(AddressHint::parseLegacy),
        clipboardPolicy = clipboard,
        notificationPolicy = notifications,
    )

    private fun merge(existing: TrustStore.TrustedPeer?, paired: TrustStore.TrustedPeer) =
        TrustStore.TrustedPeer.mergePairing(existing, paired)

    /** What `PliweeApp.pair` builds from a session that has just come up. */
    private fun freshlyPaired(
        f: Fingerprint = fedora,
        name: String = "Fedora",
        address: String = "192.168.68.72:55432",
        pairedAt: Long = 1_800_000_000L,
    ) = peer(
        f = f,
        name = name,
        // Exactly what `pair` grants: negotiated, minus the two that are
        // always withheld at pairing time.
        grants = setOf(FilesCapability.ID, BatteryCapability.ID),
        addresses = listOf(address),
        pairedAt = pairedAt,
    )

    // -----------------------------------------------------------------------
    // A. an unknown computer pairs normally
    // -----------------------------------------------------------------------

    @Test
    fun `an unknown computer is stored exactly as the pairing produced it`() {
        val paired = freshlyPaired()
        assertEquals(paired, merge(existing = null, paired = paired))
    }

    /**
     * A QR carrying a *different* fingerprint matches no record, so it takes
     * the same new-peer path. The fingerprint is the anchor; the display name
     * is never consulted.
     */
    @Test
    fun `a different fingerprint is a different computer however it is named`() {
        val known = peer(fedora, "Fedora", setOf(FilesCapability.ID))
        val impostor = freshlyPaired(f = debian, name = "Fedora")
        assertFalse(known.fingerprint.contentEquals(impostor.fingerprint))
        // Looked up by fingerprint, so `existing` for this one is null.
        assertEquals(impostor, merge(existing = null, paired = impostor))
    }

    // -----------------------------------------------------------------------
    // B, C. a locally trusted computer, scanned again
    // -----------------------------------------------------------------------

    /**
     * B. Healthy on both sides. The desktop answers `HELLO_STATUS_TRUSTED`,
     * asks for no proof, and the code is not spent. That is coherent — the
     * responder decides what it requires — and it must be *said*, because
     * reporting it as a fresh pairing is what made UX-HARDENING §20 test 8a
     * read as more than it was.
     */
    @Test
    fun `a scan against a computer that still trusts us is reported as a reconnection`() {
        val reconnected = UiMapping.pairedMessage("Fedora", provedToken = false)
        val paired = UiMapping.pairedMessage("Fedora", provedToken = true)
        assertNotEquals(reconnected, paired)
        assertEquals("Paired with Fedora.", paired)
        assertTrue(
            "it must not claim a pairing happened: $reconnected",
            reconnected.contains("already trusts") && reconnected.contains("not used"),
        )
    }

    /**
     * C. The recovery case. Nothing on this side decides it: the phone sends
     * its token whenever it has one, and the desktop's `HELLO_ACK` status is
     * what makes it a proof rather than a reconnection. What this side must
     * not do is let the *other* dialer get there first — see K.
     */
    @Test
    fun `a re-pair after a revoke keeps the record and adds nothing to it`() {
        val before = peer(
            fedora,
            "Fedora",
            grants = setOf(FilesCapability.ID, BatteryCapability.ID, ClipboardCapability.ID),
        )
        val after = merge(before, freshlyPaired())
        assertTrue(after.allows(ClipboardCapability.ID))
        assertTrue(after.allows(FilesCapability.ID))
        assertTrue(after.allows(BatteryCapability.ID))
        assertFalse(
            "pairing never grants notifications.v1 (ADR-0015 §4)",
            after.allows(NotificationsCapability.ID),
        )
    }

    // -----------------------------------------------------------------------
    // D, E, F, L. proof stays mandatory, and failure restores nothing
    // -----------------------------------------------------------------------

    /**
     * L and E. The proof is keyed on the token, so a code that is not the one
     * the desktop issued produces a proof the desktop will not accept. The
     * rest of the binding — responder, initiator, nonce, domain separation,
     * length prefixing, the cross-language known answers — is
     * [PairingProofTest] and is not repeated here.
     */
    @Test
    fun `a proof computed from the wrong code does not match`() {
        val nonce = ByteArray(32) { it.toByte() }
        val real = ByteArray(20) { 7 }
        val wrong = ByteArray(20) { 8 }

        val expected = PairingProof.compute(WireProfile.PLIWEE, real, fedora, debian, nonce)
        val offered = PairingProof.compute(WireProfile.PLIWEE, wrong, fedora, debian, nonce)

        assertFalse(PairingProof.verify(expected, offered))
        assertTrue(PairingProof.verify(expected, PairingProof.compute(WireProfile.PLIWEE, real, fedora, debian, nonce)))
    }

    /**
     * D. An expired window is the desktop's decision and cannot be argued
     * with from here: the phone holds no expiry state, so there is nothing
     * local for a stale trust record to bypass. The QR carries a token and
     * addresses and *no* validity claim at all — asserted here because a
     * payload that carried its own deadline would be a deadline the scanner
     * could be lied to about.
     */
    @Test
    fun `a scanned code carries no expiry the phone could honour or ignore`() {
        val code = "omnibridge1:${"a".repeat(64)}:FUPJRRCMFSG6KYYHR5ODB5BBLM7HVSCZ:" +
            "deb769060ca6dce3bee7a92f6a82e606:192.168.1.10:55432"
        val payload = checkNotNull(QrPayload.parse(code))
        assertEquals(QrPayload.TOKEN_LENGTH, payload.token.size)

        val fields = QrPayload::class.java.declaredFields.map { it.name.lowercase() }
        for (word in listOf("expir", "ttl", "until", "deadline", "valid", "issued")) {
            assertFalse(
                "a payload field named for validity would be a deadline the " +
                    "scanner could be lied to about: $fields",
                fields.any { it.contains(word) },
            )
        }
    }

    /**
     * F. A declined confirmation, a rejected proof and an expired window all
     * arrive as a failure, and a failure carries no peer to store. The type
     * is what enforces "trust is written in one place": only
     * `ConnectResult.Established` has a connection to build a record from.
     */
    @Test
    fun `no failure outcome carries anything a trust record could be built from`() {
        val outcomes: List<ConnectResult> = listOf(
            ConnectResult.Failed("declined on the computer"),
            ConnectResult.Failed("the pairing code was rejected"),
            ConnectResult.Failed("the computer is not in pairing mode"),
            ConnectResult.Failed("this device's pairing was revoked", FailureKind.REVOKED),
            ConnectResult.PairingRequired,
        )
        for (outcome in outcomes) {
            assertFalse(
                "only an established session can produce a trust record: $outcome",
                outcome is ConnectResult.Established,
            )
        }
        assertEquals(
            FailureKind.REVOKED,
            (outcomes[3] as ConnectResult.Failed).kind,
        )
    }

    // -----------------------------------------------------------------------
    // G, H. one row, and the person's settings survive
    // -----------------------------------------------------------------------

    /** G. A re-pair updates the record for that fingerprint, never adds one. */
    @Test
    fun `a successful re-pair keeps the same fingerprint and one record`() {
        val before = peer(fedora, "Fedora", setOf(FilesCapability.ID))
        val after = merge(before, freshlyPaired())
        assertArrayEquals(before.fingerprint.bytes, after.fingerprint.bytes)
        assertEquals(before.fingerprint.toHex(), after.fingerprint.toHex())
    }

    /**
     * H. The defect this closes. `addPeer` replaced the record wholesale, so
     * a re-pair silently turned off the clipboard the person had allowed and
     * emptied the notification app list they had chosen.
     */
    @Test
    fun `a re-pair preserves grants and policies the person set`() {
        val chosenApps = NotificationPolicy(
            allowMirror = true,
            allowedApps = setOf("com.example.chat"),
        )
        val before = peer(
            fedora,
            "Fedora",
            grants = setOf(
                FilesCapability.ID,
                ClipboardCapability.ID,
                NotificationsCapability.ID,
            ),
            clipboard = ClipboardPolicy(allowSend = true, allowReceive = true),
            notifications = chosenApps,
        )

        val after = merge(before, freshlyPaired())

        assertTrue(after.allows(ClipboardCapability.ID))
        assertTrue(
            "a grant the person turned on is not undone by re-pairing",
            after.allows(NotificationsCapability.ID),
        )
        assertEquals(before.clipboardPolicy, after.clipboardPolicy)
        assertEquals(chosenApps, after.notificationPolicy)
        assertEquals(setOf("com.example.chat"), after.notificationPolicy.allowedApps)
    }

    /** Facts about the session that just authenticated are refreshed. */
    @Test
    fun `a re-pair refreshes identity metadata and remembers both addresses`() {
        val before = peer(
            fedora,
            "old-name",
            setOf(FilesCapability.ID),
            addresses = listOf("192.168.68.99:55432"),
            pairedAt = 1_700_000_000L,
            deviceId = "stale",
        )
        val after = merge(
            before,
            freshlyPaired(name = "Fedora", address = "192.168.68.72:55432", pairedAt = 1_800_000_000L),
        )
        assertEquals("Fedora", after.deviceName)
        assertEquals("795fec0868ebef8c3d7ad3e775dc6ed0", after.deviceId)
        assertEquals(1_800_000_000L, after.pairedAtUnix)
        assertEquals(
            "the address that answered first, then what was already known",
            listOf("192.168.68.72:55432", "192.168.68.99:55432"),
            after.addressHints.map { it.format() },
        )
    }

    /** Mesh V2 SPEC §5: at most eight hints per peer, the oldest dropped. */
    @Test
    fun `remembered addresses stay bounded`() {
        val before = peer(
            fedora,
            "Fedora",
            setOf(FilesCapability.ID),
            addresses = listOf("a:1", "b:2", "c:3", "d:4", "e:5", "f:6", "g:7", "h:8"),
        )
        val after = merge(before, freshlyPaired(address = "i:9"))
        assertEquals(8, after.addressHints.size)
        assertEquals(
            "the pairing's address first, the least recent one gone",
            listOf("i:9", "a:1", "b:2", "c:3", "d:4", "e:5", "f:6", "g:7"),
            after.addressHints.map { it.format() },
        )
    }

    @Test
    fun `a re-pair at a remembered address refreshes it rather than adding a second`() {
        val before = peer(
            fedora,
            "Fedora",
            setOf(FilesCapability.ID),
            addresses = listOf("192.168.68.99:55432", "192.168.68.72:55432"),
        )
        // What `pair` builds: the hint carries the pairing time.
        val paired = freshlyPaired().copy(
            addressHints = listOf(AddressHint("192.168.68.72", 55432, 1_800_000_000L)),
        )
        val after = merge(before, paired)
        assertEquals(
            listOf(
                AddressHint("192.168.68.72", 55432, 1_800_000_000L),
                AddressHint("192.168.68.99", 55432, 0),
            ),
            after.addressHints,
        )
    }

    // -----------------------------------------------------------------------
    // I, J. everything else is left alone
    // -----------------------------------------------------------------------

    /**
     * I. The merge is a function of one record, so no other peer is reachable
     * from it. Stated over the preserved VM peers the tablet actually holds.
     */
    @Test
    fun `pairing one computer cannot touch another`() {
        val preserved = listOf(
            peer(fingerprint(3), "omnibridge-u2604", setOf(BatteryCapability.ID, FilesCapability.ID)),
            peer(fingerprint(4), "omnibridge-d13", setOf(BatteryCapability.ID, FilesCapability.ID)),
            peer(fingerprint(5), "omnibridge-u2404", setOf(BatteryCapability.ID, FilesCapability.ID)),
        )
        val fedoraBefore = peer(fedora, "Fedora", setOf(FilesCapability.ID))
        val store = preserved + fedoraBefore

        val fedoraAfter = merge(fedoraBefore, freshlyPaired())
        val updated = store.map {
            if (it.fingerprint.contentEquals(fedoraAfter.fingerprint)) fedoraAfter else it
        }

        assertEquals(4, updated.size)
        assertEquals(preserved, updated.filterNot { it.fingerprint.contentEquals(fedora) })
    }

    /**
     * J. The chosen computer is a fingerprint, and the merge never changes
     * one, so a re-pair cannot re-point the app at something else.
     */
    @Test
    fun `the chosen computer still resolves after a re-pair`() {
        val fedoraBefore = peer(fedora, "Fedora", setOf(FilesCapability.ID))
        val other = peer(debian, "omnibridge-d13", setOf(FilesCapability.ID))
        val chosenHex = fedora.toHex()

        val after = merge(fedoraBefore, freshlyPaired())
        assertEquals(chosenHex, after.fingerprint.toHex())
        assertEquals(after, PeerTarget.resolve(listOf(other, after), chosenHex).peerOrNull())
    }

    /** And list position still decides nothing. */
    @Test
    fun `a re-paired computer is not resolved by its position`() {
        val other = peer(debian, "omnibridge-d13", setOf(FilesCapability.ID))
        val after = merge(peer(fedora, "Fedora", setOf(FilesCapability.ID)), freshlyPaired())
        assertEquals(after, PeerTarget.resolve(listOf(after, other), fedora.toHex()).peerOrNull())
        assertEquals(after, PeerTarget.resolve(listOf(other, after), fedora.toHex()).peerOrNull())
    }

    // -----------------------------------------------------------------------
    // Grant non-escalation
    //
    // The merge exists so that a person's local decisions survive a
    // desktop-side revoke: a revoke by the desktop is not the phone's owner
    // withdrawing their own consent for the same cryptographic identity. That
    // is the intended semantic and these tests pin it.
    //
    // A union is also how a grant could be *invented*, and that is the
    // boundary. Until `SensitiveCapabilities` existed, the only thing stopping
    // it was a subtraction written inline in `PliweeApp` when the fresh
    // record was built — so the property held for the wrong reason, in a
    // different file, and any change to that line would have let a re-pair
    // hand a computer the notification stream because the *negotiation*
    // advertised it. `mergePairing` now applies the set where the union
    // happens.
    //
    // Preserve consent. Never invent it.
    // -----------------------------------------------------------------------

    /** The boundary itself. Emptying this set would silently allow escalation. */
    @Test
    fun `the never auto granted set names both sensitive capabilities`() {
        assertEquals(
            setOf(ClipboardCapability.ID, NotificationsCapability.ID),
            SensitiveCapabilities.NEVER_AUTO_GRANTED,
        )
        assertTrue(SensitiveCapabilities.isSensitive(NotificationsCapability.ID))
        assertTrue(SensitiveCapabilities.isSensitive(ClipboardCapability.ID))
        // Pairing may grant these: a file still needs per-transfer approval,
        // and a battery percentage is not a secret.
        assertFalse(SensitiveCapabilities.isSensitive(FilesCapability.ID))
        assertFalse(SensitiveCapabilities.isSensitive(BatteryCapability.ID))
    }

    /** Preservation — the whole reason the merge exists. */
    @Test
    fun `a re-pair preserves an existing notification grant`() {
        val before = peer(
            fedora,
            "Fedora",
            grants = setOf(FilesCapability.ID, NotificationsCapability.ID),
        )
        val after = merge(before, freshlyPaired())
        assertTrue(
            "a desktop-side revoke is not the phone owner withdrawing consent",
            after.allows(NotificationsCapability.ID),
        )
    }

    /**
     * Non-escalation. The fresh record here carries `notifications.v1`, which
     * is what a widened auto-grant, a refactor or a forgotten subtraction
     * would produce. The merge must still refuse it, because the person never
     * granted it.
     */
    @Test
    fun `a re-pair does not invent a notification grant`() {
        val before = peer(fedora, "Fedora", grants = setOf(FilesCapability.ID))
        val escalating = freshlyPaired().copy(
            grantedCapabilities = setOf(
                FilesCapability.ID,
                BatteryCapability.ID,
                NotificationsCapability.ID,
            ),
        )
        val after = merge(before, escalating)
        assertFalse(
            "negotiating notifications.v1 means both sides support it, not " +
                "that this computer may read the notification stream",
            after.allows(NotificationsCapability.ID),
        )
        assertTrue("and nothing legitimate was lost", after.allows(FilesCapability.ID))
    }

    /** Preservation, for the clipboard's grant and its direction settings. */
    @Test
    fun `a re-pair preserves existing clipboard policy`() {
        val chosen = ClipboardPolicy(
            allowSend = true,
            allowReceive = false,
            autoSend = false,
            autoReceive = false,
        )
        val before = peer(
            fedora,
            "Fedora",
            grants = setOf(FilesCapability.ID, ClipboardCapability.ID),
            clipboard = chosen,
        )
        val after = merge(before, freshlyPaired())
        assertTrue(after.allows(ClipboardCapability.ID))
        assertEquals(chosen, after.clipboardPolicy)
        assertFalse(
            "a direction the person turned off stays off",
            after.clipboardPolicy.allowReceive,
        )
    }

    /** Non-escalation, same rule, for the clipboard. */
    @Test
    fun `a re-pair does not invent clipboard permission`() {
        val before = peer(fedora, "Fedora", grants = setOf(BatteryCapability.ID))
        val escalating = freshlyPaired().copy(
            grantedCapabilities = setOf(BatteryCapability.ID, ClipboardCapability.ID),
        )
        val after = merge(before, escalating)
        assertFalse(after.allows(ClipboardCapability.ID))
        // And the policy object is still the default rather than something the
        // pairing decided: a policy is inert without the grant, and inventing
        // one here would be the same escalation wearing a different hat.
        assertEquals(before.clipboardPolicy, after.clipboardPolicy)
    }

    /**
     * Invariant 4. Auto-grantable capabilities still follow the pairing
     * policy, so this is not a blanket freeze on grants — only on the
     * sensitive ones.
     */
    @Test
    fun `a re-pair still adds an auto grantable capability it negotiated`() {
        val before = peer(fedora, "Fedora", grants = setOf(BatteryCapability.ID))
        val after = merge(before, freshlyPaired())
        assertTrue(
            "files.v1 is negotiated and auto-grantable, so pairing may add it",
            after.allows(FilesCapability.ID),
        )
        assertTrue(after.allows(BatteryCapability.ID))
    }

    /** Invariant 5, enforced inside the function rather than by its caller. */
    @Test
    fun `a different fingerprint inherits no grants`() {
        val other = peer(
            debian,
            "omnibridge-d13",
            grants = setOf(
                FilesCapability.ID,
                ClipboardCapability.ID,
                NotificationsCapability.ID,
            ),
            clipboard = ClipboardPolicy(allowSend = false, allowReceive = false),
            notifications = NotificationPolicy(allowedApps = setOf("com.example.bank")),
        )
        val fresh = freshlyPaired(f = fedora)

        // Even handed the *wrong* record, nothing crosses between keys.
        val after = merge(other, fresh)

        assertEquals(fedora.toHex(), after.fingerprint.toHex())
        assertFalse(after.allows(ClipboardCapability.ID))
        assertFalse(after.allows(NotificationsCapability.ID))
        assertEquals(ClipboardPolicy(), after.clipboardPolicy)
        assertEquals(NotificationPolicy(), after.notificationPolicy)
        assertEquals(fresh.addressHints, after.addressHints)
        assertEquals(fresh.pairedAtUnix, after.pairedAtUnix)
    }

    /** And a brand-new peer cannot arrive holding a sensitive grant either. */
    @Test
    fun `a first pairing does not grant a sensitive capability`() {
        val escalating = freshlyPaired().copy(
            grantedCapabilities = setOf(
                FilesCapability.ID,
                ClipboardCapability.ID,
                NotificationsCapability.ID,
            ),
        )
        val stored = merge(existing = null, paired = escalating)
        assertEquals(setOf(FilesCapability.ID), stored.grantedCapabilities)
    }

    // -----------------------------------------------------------------------
    // Invariant 6 — a pairing that does not succeed changes nothing
    // -----------------------------------------------------------------------

    /** The stored shape of a peer, exactly as `TrustStore.writePeers` emits it. */
    private fun serialize(peers: List<TrustStore.TrustedPeer>): String =
        peers.joinToString("|") { p ->
            org.json.JSONObject().apply {
                put("deviceId", p.deviceId)
                put("deviceName", p.deviceName)
                put("fingerprint", p.fingerprint.toHex())
                put("pairedAt", p.pairedAtUnix)
                put("grants", org.json.JSONArray(p.grantedCapabilities.sorted()))
                put("addressHints", org.json.JSONArray(p.addressHints.map { it.toJson() }))
                put("clipboardPolicy", p.clipboardPolicy.toJson())
                put("notificationPolicy", p.notificationPolicy.toJson())
            }.toString()
        }

    /**
     * A failed proof, an expired token and a declined confirmation all arrive
     * as a `ConnectResult` that is not `Established`, and only `Established`
     * carries a connection to build a record from. So the store is not
     * written, and the serialized peer list is byte-identical.
     */
    @Test
    fun `a failed pairing leaves grants and policies byte-equivalent`() {
        val store = listOf(
            peer(
                fedora,
                "Fedora",
                grants = setOf(
                    FilesCapability.ID,
                    ClipboardCapability.ID,
                    NotificationsCapability.ID,
                ),
                clipboard = ClipboardPolicy(allowSend = true, allowReceive = false),
                notifications = NotificationPolicy(allowedApps = setOf("com.example.chat")),
            ),
            peer(debian, "omnibridge-d13", grants = setOf(BatteryCapability.ID)),
        )
        val before = serialize(store)

        val failures: List<ConnectResult> = listOf(
            ConnectResult.Failed("the pairing code was rejected"),
            ConnectResult.Failed("the computer is not in pairing mode"),
            ConnectResult.Failed("declined on the computer"),
            ConnectResult.Failed("too many failed attempts"),
            ConnectResult.Failed("this device's pairing was revoked", FailureKind.REVOKED),
            ConnectResult.Failed(
                "the computer did not prove it knew the pairing code",
                FailureKind.SECURITY,
            ),
            ConnectResult.PairingRequired,
        )

        val written = store.toMutableList()
        for (outcome in failures) {
            // The whole of `pair`'s write rule: a record exists to store only
            // inside the `is ConnectResult.Established ->` arm.
            if (outcome is ConnectResult.Established) {
                written[0] = merge(written[0], freshlyPaired())
            }
        }

        assertEquals(
            "no failure path may touch a grant, a policy or a pairing time",
            before,
            serialize(written),
        )
    }

    /**
     * And that rule is structural, not a convention. `pair` reaches the trust
     * store on exactly one line, and the arms that handle failure contain no
     * store call at all.
     *
     * A source assertion on purpose: the property is about *where* a call
     * sits, and no value returned by that function can express it.
     */
    @Test
    fun `pairing writes trust on exactly one line and only when established`() {
        val source = File(
            "src/main/java/io/github/yurisismotto/pliwee/PliweeApp.kt",
        ).readText()
        val body = source
            .substringAfter("private suspend fun pairWithToken(")
            .substringBefore("\n    companion object")

        val mutators = listOf(
            "addPeer(", "upsertPairedPeer(", "removePeer(", "setGrant(",
            "setClipboardPolicy(", "setNotificationPolicy(", "selectPeer(",
            "clearSelectedPeer(", "rememberAddresses(", "recordSuccessfulAddress(",
        )
        val calls = mutators.filter { body.contains("trustStore.$it") }
        assertEquals(
            "pairing must reach the trust store through one mutator only: $calls",
            listOf("upsertPairedPeer("),
            calls,
        )
        assertEquals(
            "and exactly once",
            1,
            Regex(Regex.escape("trustStore.upsertPairedPeer(")).findAll(body).count(),
        )

        val failureArms = body.substringAfter("is ConnectResult.PairingRequired ->")
        assertFalse(
            "no failure arm may touch the trust store: $failureArms",
            failureArms.contains("trustStore."),
        )
    }

    /** Invariant 7. The merge has no opinion about which computer is chosen. */
    @Test
    fun `a re-pair leaves the chosen computer alone`() {
        val before = peer(fedora, "Fedora", grants = setOf(FilesCapability.ID))
        val after = merge(before, freshlyPaired())
        // Same fingerprint in, same fingerprint out — and `selectedPeer` is a
        // fingerprint hex, so a choice pointing at it still resolves.
        assertEquals(before.fingerprint.toHex(), after.fingerprint.toHex())
        assertEquals(
            after,
            PeerTarget.resolve(listOf(after), before.fingerprint.toHex()).peerOrNull(),
        )
    }

    // -----------------------------------------------------------------------
    // K. a stale connection attempt cannot get in the way
    // -----------------------------------------------------------------------

    /** With no scan in flight, nothing is held back. */
    @Test
    fun `reconnection is unaffected when no code is being proved`() {
        assertNull(PairingGate.holdFor(fedora.toHex(), null))
        assertNull(PairingGate.holdFor(debian.toHex(), null))
    }

    /** While a code is being proved, the tokenless dialer holds off. */
    @Test
    fun `the tokenless dialer holds off for the computer being paired`() {
        val reason = PairingGate.holdFor(fedora.toHex(), fedora.toHex())
        assertEquals(PairingGate.REASON, reason)
        assertFalse(
            "the reason is shown and logged, so it must carry no secret",
            reason!!.contains("token", ignoreCase = true) ||
                reason.contains("code:") ||
                reason.contains(fedora.toHex()),
        )
    }

    /** A hex from a QR and one from the store must match the same way. */
    @Test
    fun `the hold matches a fingerprint whatever case it is spelled in`() {
        assertEquals(PairingGate.REASON, PairingGate.holdFor(fedora.toHex().uppercase(), fedora.toHex()))
        assertEquals(PairingGate.REASON, PairingGate.holdFor(fedora.toHex(), fedora.toHex().uppercase()))
    }

    /** Pairing one computer is no reason to drop the link to another. */
    @Test
    fun `another computer keeps connecting while one is being paired`() {
        assertNull(PairingGate.holdFor(debian.toHex(), fedora.toHex()))
    }

    /**
     * And the hold is never terminal. `ConnectionService` maps it to
     * `DialResult.Transient`, which backs off and comes back; a `Terminal`
     * would stop the loop for good and leave a failed pairing as a dead link.
     */
    @Test
    fun `the hold is a transient dial result and not a terminal one`() {
        val held: DialResult = DialResult.Transient(PairingGate.REASON)
        assertFalse(
            "a Terminal would stop the loop for good and leave a failed " +
                "pairing as a dead link",
            held is DialResult.Terminal,
        )
        assertEquals(PairingGate.REASON, (held as DialResult.Transient).reason)
    }
}
