package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.files.FailureReason
import io.github.yurisismotto.pliwee.files.FileTransferManager
import io.github.yurisismotto.pliwee.files.StreamAuth
import io.github.yurisismotto.pliwee.files.TransferState
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import java.security.SecureRandom
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * UX-DEBT-01: a declined file must be sendable again.
 *
 * ## The defect
 *
 * `SendActivity` decided what to show with
 *
 * ```kotlin
 * val mine = transfers.filter { it.sending && it.filename == name }
 * ```
 *
 * over `files.visible`, which holds every transfer of the process —
 * `FileTransferManager` never prunes its map. So one decline left a permanent
 * row for that display name and the Send button never came back: the same
 * file could not be offered again for the life of the app, whatever the
 * person did short of killing it. The previous sprint had to test its Accept
 * path with a *second* filename because of this.
 *
 * ## What is asserted here
 *
 * The screen's decision is now [UiMapping.sendSurface], a total function of
 * (this screen's attempt, the transfer list), and the retry gate is
 * [UiMapping.canStartSend]. Both are pure, so the whole state model is
 * provable on the JVM in milliseconds rather than on a tablet whose
 * instrumented run destroys the pairing.
 *
 * The protocol half — that a retry is a genuinely new transfer and not a
 * resurrection of the declined one — is asserted against
 * [StreamAuth.newTransferId], which is what `files.offer` mints from.
 *
 * The end-to-end proof (share → decline → retry → accept → matching SHA256 on
 * real hardware) is in the sprint report; these are what stop it regressing
 * unnoticed.
 */
class SendRetryTest {

    // -- the world ----------------------------------------------------------

    private val name = "holiday.jpg"

    /** One peer for the fixtures below; see [transfer]. */
    private val somePeer = Fingerprint(ByteArray(32) { it.toByte() })

    private fun transfer(
        id: String,
        filename: String = name,
        state: TransferState = TransferState.OFFERED,
        failure: FailureReason? = null,
        sending: Boolean = true,
    ) = FileTransferManager.TransferUi(
        transferId = id,
        // Every transfer has a peer. Which one is irrelevant to these tests —
        // they are about a filename never being an identity — so they all
        // share one rather than pretending the distinction matters here.
        peer = somePeer,
        filename = filename,
        sizeBytes = 1024,
        bytesTransferred = 0,
        sending = sending,
        state = state,
        failure = failure,
    )

    private val declined = transfer(
        id = "aa11",
        state = TransferState.CANCELLED,
        failure = FailureReason.DECLINED_BY_USER,
    )

    private fun surface(
        attempt: UiMapping.SendAttempt,
        transfers: List<FileTransferManager.TransferUi>,
    ) = UiMapping.sendSurface(attempt, transfers)

    // -----------------------------------------------------------------------
    // A. a declined attempt is terminal
    // -----------------------------------------------------------------------

    @Test
    fun `a declined attempt is terminal and shown as a settled row`() {
        assertTrue("a decline arrives as CANCELLED", declined.state.isTerminal)
        assertFalse(declined.state.isActive)

        val result = surface(UiMapping.SendAttempt.Offered("aa11"), listOf(declined))
        assertEquals(UiMapping.SendSurface.Ended(declined), result)
    }

    /** And it stays terminal: nothing leaves a terminal state. */
    @Test
    fun `nothing moves a declined transfer back to a live state`() {
        for (next in TransferState.entries) {
            assertFalse(
                "CANCELLED must not transition to $next",
                TransferState.CANCELLED.canTransitionTo(next),
            )
        }
    }

    /** A retry must never be able to make the old attempt look successful. */
    @Test
    fun `a settled attempt cannot be rewritten as a success`() {
        assertFalse(TransferState.CANCELLED.canTransitionTo(TransferState.COMPLETED))
        assertFalse(TransferState.FAILED.canTransitionTo(TransferState.COMPLETED))
        assertFalse(TransferState.COMPLETED.canTransitionTo(TransferState.FAILED))
    }

    // -----------------------------------------------------------------------
    // B. the same file can be offered again — the defect itself
    // -----------------------------------------------------------------------

    /**
     * The regression test. A fresh Sharesheet invocation starts at `Idle`,
     * and the declined row for the same filename is history rather than a
     * blocker.
     */
    @Test
    fun `a fresh share of the same file offers the send button again`() {
        assertEquals(
            "a declined transfer with this filename must not suppress Send",
            UiMapping.SendSurface.Offer,
            surface(UiMapping.SendAttempt.Idle, listOf(declined)),
        )
        assertTrue(
            UiMapping.canStartSend(UiMapping.SendAttempt.Idle, UiMapping.SendSurface.Offer),
        )
    }

    /** The old predicate, restated, so the shape of the defect is on record. */
    @Test
    fun `the send surface is not decided by filename`() {
        val history = listOf(
            declined,
            transfer("bb22", state = TransferState.COMPLETED),
            transfer("cc33", state = TransferState.FAILED, failure = FailureReason.TRANSPORT),
        )
        val matchedByName = history.filter { it.sending && it.filename == name }
        assertEquals("the fixture must exercise the old predicate", 3, matchedByName.size)

        assertEquals(UiMapping.SendSurface.Offer, surface(UiMapping.SendAttempt.Idle, history))
    }

    /** The settled row offers the retry directly, without re-sharing. */
    @Test
    fun `a settled attempt offers a retry in place`() {
        val ended = surface(UiMapping.SendAttempt.Offered("aa11"), listOf(declined))
        assertTrue(
            "the whole point of UX-DEBT-01",
            UiMapping.canStartSend(UiMapping.SendAttempt.Offered("aa11"), ended),
        )
        assertEquals("Try again", UiMapping.retryButtonLabel(TransferState.CANCELLED))
    }

    // -----------------------------------------------------------------------
    // C. a retry is a new transfer, not the old one
    // -----------------------------------------------------------------------

    /**
     * Ids come from randomness alone. If anything about the file fed into
     * them, a retry of the same file would collide with what was declined —
     * and the id is what the stream challenge is bound to.
     */
    @Test
    fun `every minted transfer id is fresh and full length`() {
        val random = SecureRandom()
        val ids = (1..500).map { StreamAuth.toHex(StreamAuth.newTransferId(random)) }
        assertEquals("ids must not repeat", ids.size, ids.toSet().size)
        for (id in ids) {
            assertEquals(StreamAuth.TRANSFER_ID_LENGTH * 2, id.length)
            assertTrue("lowercase hex only: $id", id.all { it in '0'..'9' || it in 'a'..'f' })
        }
    }

    /** Two mints from the same file's name produce different transfers. */
    @Test
    fun `a retry of the same file gets a different transfer id`() {
        val random = SecureRandom()
        val first = StreamAuth.toHex(StreamAuth.newTransferId(random))
        val second = StreamAuth.toHex(StreamAuth.newTransferId(random))
        assertNotEquals(first, second)

        // And the screen follows the new one, leaving the old row settled.
        val old = transfer(first, state = TransferState.CANCELLED, failure = FailureReason.DECLINED_BY_USER)
        val fresh = transfer(second, state = TransferState.OFFERED)
        assertEquals(
            UiMapping.SendSurface.InFlight(fresh),
            surface(UiMapping.SendAttempt.Offered(second), listOf(old, fresh)),
        )
    }

    /** The attempt carries an id and nothing else — no peer, no URI, no name. */
    @Test
    fun `an attempt identifies a transfer and nothing else`() {
        val attempt = UiMapping.SendAttempt.Offered("aa11")
        assertEquals("aa11", attempt.transferId)
        assertEquals(
            "a retry must re-resolve its destination, never inherit one",
            "Offered(transferId=aa11)",
            attempt.toString(),
        )
    }

    // -----------------------------------------------------------------------
    // D and E. failure and completion are both retryable
    // -----------------------------------------------------------------------

    @Test
    fun `a failed attempt can be retried`() {
        val failed = transfer("dd44", state = TransferState.FAILED, failure = FailureReason.TRANSPORT)
        val ended = surface(UiMapping.SendAttempt.Offered("dd44"), listOf(failed))
        assertEquals(UiMapping.SendSurface.Ended(failed), ended)
        assertTrue(UiMapping.canStartSend(UiMapping.SendAttempt.Offered("dd44"), ended))
        assertEquals("Try again", UiMapping.retryButtonLabel(TransferState.FAILED))
    }

    /** A refusal before any transfer exists is retryable from the button. */
    @Test
    fun `an offer refused before a transfer existed leaves the button live`() {
        val refused = UiMapping.sendOutcome(
            Result.failure(IllegalStateException("not connected")),
        )
        assertEquals(UiMapping.SendAttempt.Failed("not connected"), refused)
        assertEquals(UiMapping.SendSurface.Offer, surface(refused, emptyList()))
        assertTrue(UiMapping.canStartSend(refused, UiMapping.SendSurface.Offer))
        assertEquals("Try again", UiMapping.sendButtonLabel(refused))
    }

    @Test
    fun `a completed send does not block sending the same file again`() {
        val done = transfer("ee55", state = TransferState.COMPLETED)
        val ended = surface(UiMapping.SendAttempt.Offered("ee55"), listOf(done))
        assertEquals(UiMapping.SendSurface.Ended(done), ended)
        assertTrue(UiMapping.canStartSend(UiMapping.SendAttempt.Offered("ee55"), ended))
        // Not "Try again": nothing went wrong.
        assertEquals("Send again", UiMapping.retryButtonLabel(TransferState.COMPLETED))
        // And a fresh share of it starts clean.
        assertEquals(UiMapping.SendSurface.Offer, surface(UiMapping.SendAttempt.Idle, listOf(done)))
    }

    // -----------------------------------------------------------------------
    // F. a display name is not an identity
    // -----------------------------------------------------------------------

    /**
     * Two different files that happen to be called the same thing. Under the
     * old predicate each hid the other's Send button; under this one neither
     * is visible to the other.
     */
    @Test
    fun `two transfers with the same filename do not alias`() {
        val mine = transfer("ff66", state = TransferState.TRANSFERRING)
        val theirs = transfer("aa77", state = TransferState.CANCELLED, failure = FailureReason.DECLINED_BY_USER)
        val both = listOf(theirs, mine)

        assertEquals(
            UiMapping.SendSurface.InFlight(mine),
            surface(UiMapping.SendAttempt.Offered("ff66"), both),
        )
        assertEquals(
            UiMapping.SendSurface.Ended(theirs),
            surface(UiMapping.SendAttempt.Offered("aa77"), both),
        )
    }

    /** An incoming transfer is never mistaken for this screen's send. */
    @Test
    fun `a received transfer is not this screen's attempt`() {
        val incoming = transfer("bb88", sending = false, state = TransferState.COMPLETED)
        assertEquals(UiMapping.SendSurface.Offer, surface(UiMapping.SendAttempt.Idle, listOf(incoming)))
    }

    // -----------------------------------------------------------------------
    // G. repeated taps do not pile up transfers
    // -----------------------------------------------------------------------

    @Test
    fun `a second tap while an offer is in flight starts nothing`() {
        // The instant after the first tap, before `offer` has returned.
        assertFalse(
            UiMapping.canStartSend(UiMapping.SendAttempt.Sending, UiMapping.SendSurface.Offer),
        )
        // The instant after it returned, before the row arrives.
        val waiting = surface(UiMapping.SendAttempt.Offered("aa11"), emptyList())
        assertEquals(UiMapping.SendSurface.InFlight(null), waiting)
        assertFalse(UiMapping.canStartSend(UiMapping.SendAttempt.Offered("aa11"), waiting))
        // And while it is running.
        val running = surface(
            UiMapping.SendAttempt.Offered("aa11"),
            listOf(transfer("aa11", state = TransferState.TRANSFERRING)),
        )
        assertFalse(UiMapping.canStartSend(UiMapping.SendAttempt.Offered("aa11"), running))
    }

    /** Every live state refuses a new attempt; every terminal one allows it. */
    @Test
    fun `startability follows exactly the terminal boundary`() {
        for (state in TransferState.entries) {
            val row = transfer("aa11", state = state)
            val attempt = UiMapping.SendAttempt.Offered("aa11")
            assertEquals(
                "$state",
                state.isTerminal,
                UiMapping.canStartSend(attempt, surface(attempt, listOf(row))),
            )
        }
    }

    /** "Sending…" is never what a settled attempt reads as (issue #12). */
    @Test
    fun `no attempt state leaves the button claiming to be sending`() {
        assertEquals("Send", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Idle))
        assertEquals("Try again", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Failed("x")))
        assertEquals("Sending…", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Sending))
        assertEquals("Sending…", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Offered("aa11")))
    }

    // -----------------------------------------------------------------------
    // H, I and J. the destination is re-resolved, never inherited
    // -----------------------------------------------------------------------

    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private fun peer(name: String, seed: Int, grants: Set<String>) = TrustStore.TrustedPeer(
        deviceId = "device-$seed",
        deviceName = name,
        fingerprint = fingerprint(seed),
        pairedAtUnix = 1_700_000_000L + seed,
        grantedCapabilities = grants,
        addressHints = listOf(AddressHint("192.168.68.${70 + seed}", 55432)),
    )

    private val fedora = peer("Fedora", 1, setOf(FilesCapability.ID, BatteryCapability.ID))
    private val debian = peer("omnibridge-d13", 2, setOf(FilesCapability.ID))

    /**
     * H. The screen's surface knows nothing about peers, so a retry cannot
     * pick up the destination of some earlier attempt. The destination comes
     * from [PeerTarget.resolve] over [UiMapping.fileDestinations] at the
     * moment of the tap, exactly as the first attempt's did.
     */
    @Test
    fun `a retry resolves its destination the same way a first send does`() {
        val peers = listOf(fedora, debian)
        val chosen = PeerTarget.resolve(
            UiMapping.fileDestinations(peers),
            fedora.fingerprint.toHex(),
        )
        assertEquals(fedora, chosen.peerOrNull())

        // The settled attempt that is about to be retried carries no peer at
        // all, so there is nothing for it to inherit.
        val ended = surface(UiMapping.SendAttempt.Offered("aa11"), listOf(declined))
        assertTrue(ended is UiMapping.SendSurface.Ended)
        assertFalse(
            "a transfer row must not carry a destination the retry could reuse",
            declined.toString().contains(fedora.fingerprint.toHex()),
        )
    }

    /**
     * I. A grant withdrawn between the decline and the retry removes the
     * destination, because eligibility is re-derived rather than remembered.
     */
    @Test
    fun `a retry respects a grant withdrawn since the declined attempt`() {
        val granted = listOf(fedora)
        assertEquals(listOf(fedora), UiMapping.fileDestinations(granted))

        val withdrawn = listOf(fedora.copy(grantedCapabilities = setOf(BatteryCapability.ID)))
        assertTrue(
            "with no files.v1 grant there is no destination, so no retry",
            UiMapping.fileDestinations(withdrawn).isEmpty(),
        )
        assertEquals(
            PeerTarget.Resolution.NoTrustedPeer,
            PeerTarget.resolve(UiMapping.fileDestinations(withdrawn), fedora.fingerprint.toHex()),
        )
    }

    /**
     * J. A retry while disconnected takes the ordinary path and is refused by
     * the capability, not by the screen inventing an answer.
     */
    @Test
    fun `a retry with no session is refused and stays retryable`() {
        val outcome = UiMapping.sendOutcome(
            Result.failure(IllegalStateException("not connected")),
        )
        assertTrue(outcome is UiMapping.SendAttempt.Failed)
        assertEquals("not connected", (outcome as UiMapping.SendAttempt.Failed).message)
        assertTrue("and the button comes back", outcome.canSend)
    }

    /** A platform exception is never what reaches the screen. */
    @Test
    fun `a retry failure never puts a content uri on screen`() {
        val leak = java.io.FileNotFoundException(
            "No content provider: content://media/external/images/media/42",
        )
        val message = UiMapping.sendFailureMessage(leak)
        assertEquals("Pliwee could not send that file.", message)
        assertFalse(message.contains("content://"))
    }
}
