package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Which computer a connection or a share is aimed at.
 *
 * ## The defect these protect
 *
 * Certified on real hardware, U2 §39.17. A tablet trusted two desktops:
 *
 * ```text
 * [0] omnibridge-u2404   trusted, POWERED OFF
 * [1] omnibridge-d13     trusted, running, reachable, pinging fine
 * ```
 *
 * `ConnectionService` resolved its destination as `peers().firstOrNull()`, so
 * every attempt went to `[0]`. The Debian daemon logged **zero** connection
 * attempts across an explicit Connect, a full app restart and eighty seconds
 * of waiting. Deleting the Ubuntu *trust entry* — the Ubuntu machine itself
 * untouched — and Debian connected in seven seconds.
 *
 * So the property under test is not "does it pick a peer" but **"can list
 * position influence the answer"**. Every test below therefore asserts on a
 * *set* of peers and a chosen fingerprint, and the order-reversal tests exist
 * to fail loudly if `first()` ever creeps back under another name.
 */
class PeerTargetTest {

    // -- fixtures -----------------------------------------------------------

    /** Distinct, deterministic, and nothing to do with the real fixtures' order. */
    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private fun peer(
        name: String,
        seed: Int,
        grants: Set<String> = setOf(FilesCapability.ID, ClipboardCapability.ID),
        clipboard: ClipboardPolicy = ClipboardPolicy(allowSend = true),
    ) = TrustStore.TrustedPeer(
        deviceId = "device-$seed",
        deviceName = name,
        fingerprint = fingerprint(seed),
        pairedAtUnix = 1_700_000_000L + seed,
        grantedCapabilities = grants,
        addressHints = listOf(AddressHint("192.168.68.${70 + seed}", 55432)),
        clipboardPolicy = clipboard,
    )

    /** Ubuntu 24.04: trusted, and powered off for the whole of the U2 session. */
    private val ubuntu = peer("omnibridge-u2404", 1)

    /** Debian 13: trusted, running, and never once dialled. */
    private val debian = peer("omnibridge-d13", 2)

    /** Ubuntu 26.04: the third certified guest. */
    private val ubuntu26 = peer("omnibridge-u2604", 3)

    private fun hex(peer: TrustStore.TrustedPeer) = peer.fingerprint.toHex()

    // -- T1  the certified reproduction -------------------------------------

    @Test
    fun `T1 an offline first trust entry does not block a later selected peer`() {
        // The exact trust store from the certified failure, in the order it
        // was captured: Ubuntu first and offline, Debian second and reachable.
        val store = listOf(ubuntu, debian)

        val resolved = PeerTarget.resolve(store, hex(debian))

        assertEquals(debian, resolved.peerOrNull())
        assertNull("a resolved target is not blocked", resolved.blockedReason())
    }

    @Test
    fun `T2 reversing the trust store order changes nothing`() {
        val forwards = PeerTarget.resolve(listOf(ubuntu, debian), hex(debian))
        val backwards = PeerTarget.resolve(listOf(debian, ubuntu), hex(debian))

        // Same externally observable result from both orders. This is the
        // assertion `first()` cannot pass.
        assertEquals(debian, forwards.peerOrNull())
        assertEquals(debian, backwards.peerOrNull())
        assertEquals(forwards.peerOrNull(), backwards.peerOrNull())
    }

    @Test
    fun `T3 selecting the first entry also works, in both orders`() {
        // The mirror of T2. A fix that merely preferred the *last* peer, or
        // the second, would pass T2 and fail here.
        assertEquals(ubuntu, PeerTarget.resolve(listOf(ubuntu, debian), hex(ubuntu)).peerOrNull())
        assertEquals(ubuntu, PeerTarget.resolve(listOf(debian, ubuntu), hex(ubuntu)).peerOrNull())
    }

    // -- T4  three peers ----------------------------------------------------

    @Test
    fun `T4 with three trusted peers the chosen one is the target, whatever the order`() {
        val orders = listOf(
            listOf(ubuntu, debian, ubuntu26),
            listOf(ubuntu26, ubuntu, debian),
            listOf(debian, ubuntu26, ubuntu),
        )
        for (order in orders) {
            assertEquals(
                "selecting omnibridge-u2604 in order ${order.map { it.deviceName }}",
                ubuntu26,
                PeerTarget.resolve(order, hex(ubuntu26)).peerOrNull(),
            )
        }
    }

    @Test
    fun `T5 every peer is reachable as a target from the same store`() {
        // Not one privileged entry and two unreachable ones: a three-peer
        // store must be able to name any of the three.
        val store = listOf(ubuntu, debian, ubuntu26)
        for (wanted in store) {
            assertEquals(wanted, PeerTarget.resolve(store, hex(wanted)).peerOrNull())
        }
    }

    // -- T6  no explicit choice ---------------------------------------------

    @Test
    fun `T6 one trusted peer needs no choice`() {
        val resolved = PeerTarget.resolve(listOf(debian), selectedHex = null)
        assertEquals(debian, resolved.peerOrNull())
        assertTrue(resolved is PeerTarget.Resolution.OnlyTrustedPeer)
    }

    @Test
    fun `T7 several trusted peers and no choice resolves to nothing, not to the first`() {
        val resolved = PeerTarget.resolve(listOf(ubuntu, debian), selectedHex = null)

        assertNull("the first entry must not become the target by default", resolved.peerOrNull())
        assertTrue(resolved is PeerTarget.Resolution.MustChoose)
        assertEquals("choose which computer to connect to", resolved.blockedReason())
        // The picker gets every candidate, not a truncated or reordered set.
        assertEquals(listOf(ubuntu, debian), resolved.candidates)
    }

    @Test
    fun `T8 no trusted peer is stated as such, not as an ambiguity`() {
        val resolved = PeerTarget.resolve(emptyList(), selectedHex = null)
        assertNull(resolved.peerOrNull())
        assertEquals("no paired computer", resolved.blockedReason())
        assertTrue(resolved.candidates.isEmpty())
    }

    // -- T9  identity, not name or position ---------------------------------

    @Test
    fun `T9 a choice naming an untrusted fingerprint is ignored, never honoured`() {
        val stranger = peer("not-paired", 9)
        val resolved = PeerTarget.resolve(listOf(ubuntu, debian), hex(stranger))

        // It falls back to the no-choice rule — which with two peers means
        // asking — rather than silently connecting to the stranger, and rather
        // than silently connecting to `first()` either.
        assertNull(resolved.peerOrNull())
        assertTrue(resolved is PeerTarget.Resolution.MustChoose)
    }

    @Test
    fun `T10 a forgotten peer's stale choice falls back to the remaining one`() {
        // Ubuntu was chosen, then forgotten. One computer is left, so there is
        // nothing to be ambiguous about and the app must not deadlock.
        val resolved = PeerTarget.resolve(listOf(debian), hex(ubuntu))
        assertEquals(debian, resolved.peerOrNull())
    }

    @Test
    fun `T11 two computers sharing a display name are told apart by fingerprint`() {
        val twinA = peer("desktop", 4)
        val twinB = peer("desktop", 5)
        assertEquals(twinB, PeerTarget.resolve(listOf(twinA, twinB), hex(twinB)).peerOrNull())
        assertEquals(twinA, PeerTarget.resolve(listOf(twinA, twinB), hex(twinA)).peerOrNull())
    }

    @Test
    fun `T12 an uppercase or truncated hex selects nothing`() {
        // `Fingerprint.toHex` is lowercase and 64 characters. One identity has
        // exactly one spelling, so a near-miss must not match by accident.
        val store = listOf(ubuntu, debian)
        assertNull(PeerTarget.resolve(store, hex(debian).uppercase()).peerOrNull())
        assertNull(PeerTarget.resolve(store, hex(debian).take(32)).peerOrNull())
    }

    // -- T13  share destinations --------------------------------------------

    @Test
    fun `T13 a share destination is resolved over the eligible peers only`() {
        val noFiles = peer("no-files", 6, grants = setOf(ClipboardCapability.ID))
        val resolved = PeerTarget.resolveFor(listOf(noFiles, debian), selectedHex = null) {
            it.allows(FilesCapability.ID)
        }
        // One eligible computer, so no choice is needed — even though the
        // trust store holds two peers and the ineligible one is first.
        assertEquals(debian, resolved.peerOrNull())
    }

    @Test
    fun `T14 several eligible share destinations are never resolved by trust order`() {
        val resolved = PeerTarget.resolveFor(listOf(ubuntu, debian), selectedHex = null) {
            it.allows(FilesCapability.ID)
        }
        assertNull(resolved.peerOrNull())
        assertEquals(listOf(ubuntu, debian), resolved.candidates)
    }

    @Test
    fun `T15 an explicitly chosen share destination wins over trust order`() {
        val resolved = PeerTarget.resolveFor(listOf(ubuntu, debian), hex(debian)) {
            it.allows(FilesCapability.ID)
        }
        assertEquals(debian, resolved.peerOrNull())
    }

    @Test
    fun `T16 zero eligible share destinations is stated, not guessed at`() {
        val resolved = PeerTarget.resolveFor(listOf(ubuntu, debian), hex(debian)) { false }
        assertNull(resolved.peerOrNull())
        assertEquals("no paired computer", resolved.blockedReason())
    }

    @Test
    fun `T17 a chosen peer that is not eligible for this share does not become the target`() {
        // Debian is the connection target, but its clipboard send is off. The
        // text share must not be routed to it just because it is "current".
        val clipboardOff = debian.copy(clipboardPolicy = ClipboardPolicy(allowSend = false))
        val resolved = PeerTarget.resolveFor(listOf(ubuntu, clipboardOff), hex(clipboardOff)) {
            it.allows(ClipboardCapability.ID) && it.clipboardPolicy.allowSend
        }
        // Ubuntu is the only eligible one left, and is used because it is the
        // only one — not because it is first.
        assertEquals(ubuntu, resolved.peerOrNull())
        assertTrue(resolved is PeerTarget.Resolution.OnlyTrustedPeer)
    }

    // -- T18  the shape of the answer ---------------------------------------

    @Test
    fun `T18 an explicit choice is reported as explicit`() {
        // The distinction matters to the UI: "you chose this" and "it is the
        // only one" read differently, and conflating them is how a person
        // comes to believe they picked something they never did.
        val explicit = PeerTarget.resolve(listOf(ubuntu, debian), hex(debian))
        val implicit = PeerTarget.resolve(listOf(debian), selectedHex = null)
        assertTrue(explicit is PeerTarget.Resolution.Selected)
        assertTrue(implicit is PeerTarget.Resolution.OnlyTrustedPeer)
        assertFalse(implicit is PeerTarget.Resolution.Selected)
    }
}
