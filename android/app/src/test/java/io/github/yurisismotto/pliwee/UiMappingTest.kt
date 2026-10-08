package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.files.TransferState
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import io.github.yurisismotto.pliwee.ui.theme.PliweeStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The rules the UI applies, tested without a device.
 *
 * These are the assertions that would otherwise need an instrumented run —
 * and an instrumented run uninstalls the app, which destroys the Keystore key
 * and costs a manual re-pairing every time. Keeping the decisions in plain
 * functions is what makes them cheap to check.
 */
class UiMappingTest {

    private fun peer(
        vararg capabilities: String,
        policy: ClipboardPolicy = ClipboardPolicy(),
    ) = TrustStore.TrustedPeer(
        deviceId = "d0",
        deviceName = "Fedora",
        fingerprint = Fingerprint.fromHex("00".repeat(32))!!,
        pairedAtUnix = 0,
        grantedCapabilities = capabilities.toSet(),
        addressHints = emptyList(),
        clipboardPolicy = policy,
    )

    // ---- status mapping --------------------------------------------------

    @Test
    fun `a live session reads as connected`() {
        assertEquals(
            PliweeStatus.Connected,
            UiMapping.statusFor(PliweeApp.ConnectionState.Idle, connected = true),
        )
    }

    @Test
    fun `a paired but idle peer reads as available, not disconnected`() {
        // "Disconnected" reads as a fault. A paired device with no session is
        // simply resting, and the card should not imply something is wrong.
        assertEquals(
            PliweeStatus.Available,
            UiMapping.statusFor(PliweeApp.ConnectionState.Idle, connected = false),
        )
    }

    @Test
    fun `retrying reads as connecting rather than failed`() {
        assertEquals(
            PliweeStatus.Connecting,
            UiMapping.statusFor(
                PliweeApp.ConnectionState.Retrying("network", 3),
                connected = false,
            ),
        )
    }

    @Test
    fun `an error state is surfaced as an error`() {
        assertEquals(
            PliweeStatus.Error,
            UiMapping.statusFor(PliweeApp.ConnectionState.Error("boom"), connected = false),
        )
    }

    @Test
    fun `a peer nobody is dialling does not borrow the link state`() {
        // U2 §39.17: with two paired desktops the card for the one that was
        // never being attempted read "Connecting…" exactly like the one that
        // was, which is how a defect in routing became invisible. A computer
        // that is not the target is resting, whatever the link is doing.
        for (state in listOf(
            PliweeApp.ConnectionState.Connecting,
            PliweeApp.ConnectionState.Retrying("network", 3),
            PliweeApp.ConnectionState.Error("boom"),
        )) {
            assertEquals(
                "untargeted peer under $state",
                PliweeStatus.Available,
                UiMapping.statusFor(state, connected = false, targeted = false),
            )
        }
    }

    @Test
    fun `the targeted peer still shows what the link is doing`() {
        // The mirror: silencing the untargeted card must not silence the one
        // the person is actually waiting on.
        assertEquals(
            PliweeStatus.Connecting,
            UiMapping.statusFor(
                PliweeApp.ConnectionState.Retrying("network", 3),
                connected = false,
                targeted = true,
            ),
        )
        assertEquals(
            PliweeStatus.Error,
            UiMapping.statusFor(
                PliweeApp.ConnectionState.Error("boom"),
                connected = false,
                targeted = true,
            ),
        )
    }

    @Test
    fun `a live session reads as connected even for an untargeted peer`() {
        // Defensive: a session that exists is a fact, and must never be
        // hidden by a targeting flag that has drifted.
        assertEquals(
            PliweeStatus.Connected,
            UiMapping.statusFor(
                PliweeApp.ConnectionState.Idle,
                connected = true,
                targeted = false,
            ),
        )
    }

    // ---- capability and policy gating ------------------------------------

    /** A live session with this peer, negotiating [capabilities]. */
    private fun session(vararg capabilities: String) = PliweeApp.LiveSession(
        peerHex = "00".repeat(32),
        negotiated = capabilities.toSet(),
    )

    @Test
    fun `sending the clipboard needs the grant, the direction, a session and negotiation`() {
        val full = peer(ClipboardCapability.ID)
        val live = session(ClipboardCapability.ID)
        assertTrue(UiMapping.canSendClipboard(full, live))

        // Any one of the four missing is enough to disable the action.
        assertFalse(
            "a session alone is not authorisation",
            UiMapping.canSendClipboard(peer(), live),
        )
        assertFalse(
            "the grant alone is not a connection",
            UiMapping.canSendClipboard(full, null),
        )
        assertFalse(
            "the grant does not override the send direction being off",
            UiMapping.canSendClipboard(
                peer(ClipboardCapability.ID, policy = ClipboardPolicy(allowSend = false)),
                live,
            ),
        )
        assertFalse(
            "a session that never negotiated the clipboard cannot carry one",
            UiMapping.canSendClipboard(full, session(BatteryCapability.ID)),
        )
    }

    @Test
    fun `a battery grant does not authorise the clipboard`() {
        // Capabilities are independent. Granting one must never widen another.
        assertFalse(
            UiMapping.canSendClipboard(
                peer(BatteryCapability.ID),
                session(ClipboardCapability.ID),
            ),
        )
        assertFalse(UiMapping.canSendFiles(peer(BatteryCapability.ID), connected = true))
    }

    @Test
    fun `sending files needs the files grant and a session`() {
        assertTrue(UiMapping.canSendFiles(peer(FilesCapability.ID), connected = true))
        assertFalse(UiMapping.canSendFiles(peer(FilesCapability.ID), connected = false))
        assertFalse(UiMapping.canSendFiles(peer(ClipboardCapability.ID), connected = true))
    }

    @Test
    fun `auto-apply cannot be enabled while receiving is off`() {
        assertTrue(UiMapping.autoReceiveEnabled(ClipboardPolicy(allowReceive = true)))
        assertFalse(UiMapping.autoReceiveEnabled(ClipboardPolicy(allowReceive = false)))
    }

    @Test
    fun `no auto-send switch is offered on this platform`() {
        // Offering a toggle that cannot work is worse than not offering one:
        // the person turns it on and quietly gets nothing.
        assertFalse(UiMapping.autoSendOfferable())
    }

    @Test
    fun `the receive behaviour is described accurately for each policy`() {
        assertTrue(
            UiMapping.receiveBehaviour(ClipboardPolicy(allowReceive = false))
                .contains("refused"),
        )
        assertTrue(
            UiMapping.receiveBehaviour(
                ClipboardPolicy(allowReceive = true, autoReceive = true),
            ).contains("replaces"),
        )
        assertTrue(
            UiMapping.receiveBehaviour(
                ClipboardPolicy(allowReceive = true, autoReceive = false),
            ).contains("waits"),
        )
    }

    // ---- transfer states -------------------------------------------------

    @Test
    fun `a cancelled transfer is not shown as a failure`() {
        assertNotEquals(
            PliweeStatus.Error,
            UiMapping.transferStatus(TransferState.CANCELLED),
        )
        assertEquals(PliweeStatus.Error, UiMapping.transferStatus(TransferState.FAILED))
    }

    @Test
    fun `every transfer state maps to a status`() {
        TransferState.entries.forEach { state ->
            // An unmapped state would throw; this is what stops a new one from
            // being added without deciding how it reads.
            UiMapping.transferStatus(state)
        }
    }

    // ---- the status language itself --------------------------------------

    @Test
    fun `no status is carried by colour alone`() {
        PliweeStatus.entries.forEach { status ->
            assertTrue(
                "${status.name} has no label",
                status.label.isNotBlank(),
            )
            assertTrue(
                "${status.name} has no icon",
                status.icon != 0,
            )
        }
    }

    // ---- lazy list keys --------------------------------------------------

    /**
     * The regression this file exists for.
     *
     * A pending clip is identified by the fingerprint of the peer it came
     * from; a device card by the fingerprint of the peer it *is*. Before the
     * keys were namespaced those collided, Compose threw
     * `IllegalArgumentException: Key ... was already used`, and the home
     * screen crashed on every single received clip. Caught on hardware, not
     * in a test — so here is the test.
     */
    @Test
    fun `a clip and its own peer do not share a list key`() {
        val fingerprint = "df65d3e4ba28edf9"
        assertNotEquals(
            UiMapping.clipKey(fingerprint),
            UiMapping.peerKey(fingerprint),
        )
    }

    @Test
    fun `an offer and a transfer with the same id do not share a list key`() {
        // An incoming offer becomes a transfer, and both can be on screen at
        // once carrying the same transfer id.
        val id = "0123abcd"
        assertNotEquals(UiMapping.offerKey(id), UiMapping.transferKey(id))
    }

    @Test
    fun `every key kind stays distinct for one identifier`() {
        val id = "same-identifier"
        val keys = listOf(
            UiMapping.offerKey(id),
            UiMapping.clipKey(id),
            UiMapping.peerKey(id),
            UiMapping.transferKey(id),
        )
        assertEquals(
            "each kind of list item must produce its own key",
            keys.size,
            keys.toSet().size,
        )
    }

    @Test
    fun `destructive actions are declared in one place`() {
        assertTrue(UiMapping.DESTRUCTIVE_ACTIONS.contains("forget_device"))
    }

    // -----------------------------------------------------------------------
    // The Sharesheet send outcome (issue #12)
    //
    // The defect these guard: `SendActivity.startSend` discarded the `Result`
    // of `files.offer`, so an offer that was refused before a transfer row
    // existed left the button on "Sending…" with nothing on the way. These
    // pin the property that makes that impossible — every `Result` maps to a
    // state that is not `Sending`.
    // -----------------------------------------------------------------------

    @Test
    fun `a successful offer leaves the sending state`() {
        val outcome = UiMapping.sendOutcome(Result.success("0123abcd"))
        // UX-DEBT-01: the id `offer` returned is kept, because that is what
        // the screen follows instead of matching on the display filename.
        assertEquals(UiMapping.SendAttempt.Offered("0123abcd"), outcome)
        assertNotEquals(UiMapping.SendAttempt.Sending, outcome)
    }

    @Test
    fun `a failed offer leaves the sending state and says why`() {
        val outcome = UiMapping.sendOutcome(
            Result.failure(IllegalStateException("not connected")),
        )
        assertEquals(UiMapping.SendAttempt.Failed("not connected"), outcome)
        assertNotEquals(UiMapping.SendAttempt.Sending, outcome)
    }

    @Test
    fun `a failure can be retried from`() {
        val outcome = UiMapping.sendOutcome(
            Result.failure(IllegalStateException("too many transfers at once")),
        )
        assertTrue(
            "a failed attempt must re-enable the Send button, or the screen " +
                "is the dead end issue #12 described",
            outcome.canSend,
        )
    }

    @Test
    fun `an attempt in flight cannot be started a second time`() {
        // Double-tap and re-entry: only Idle and Failed release the button.
        assertFalse(UiMapping.SendAttempt.Sending.canSend)
        assertFalse(UiMapping.SendAttempt.Sent.canSend)
        assertFalse(UiMapping.SendAttempt.Offered("0123abcd").canSend)
        assertTrue(UiMapping.SendAttempt.Idle.canSend)
        assertTrue(UiMapping.SendAttempt.Failed("nope").canSend)
    }

    @Test
    fun `no offer result can leave the screen sending`() {
        // The exhaustive statement of the bug: whatever `offer` returns, the
        // state it produces is one a person can act on.
        val results = listOf(
            Result.success("id"),
            Result.failure(IllegalStateException("this computer is not allowed to receive files")),
            Result.failure(IllegalStateException("not connected")),
            Result.failure(IllegalStateException("too many transfers at once")),
            Result.failure(IllegalStateException("that file has no name Pliwee can send safely")),
            Result.failure(IllegalStateException("that file could not be read")),
            Result.failure(java.io.IOException("the app that shared this file would not open it")),
            Result.failure(SecurityException("Permission Denial")),
        )
        for (result in results) {
            val outcome = UiMapping.sendOutcome(result)
            assertNotEquals(
                "a $result must not leave the screen on Sending",
                UiMapping.SendAttempt.Sending,
                outcome,
            )
            assertTrue(
                "a $result must reach a state the user can see",
                outcome is UiMapping.SendAttempt.Offered ||
                    outcome is UiMapping.SendAttempt.Failed,
            )
        }
    }

    @Test
    fun `the capability's own refusals are shown as written`() {
        // These are authored for a person and carry no file data, so they are
        // not replaced by a vaguer message.
        for (stated in listOf(
            "this computer is not allowed to receive files",
            "not connected",
            "too many transfers at once",
            "that file has no name Pliwee can send safely",
            "that file could not be read",
        )) {
            assertEquals(stated, UiMapping.sendFailureMessage(IllegalStateException(stated)))
        }
    }

    @Test
    fun `a platform error never puts the shared file on screen`() {
        // The real shapes: a content provider names the URI in its message,
        // and a permission denial names the provider and often the document
        // id with it. Neither may be shown or logged.
        val leaky = listOf(
            java.io.FileNotFoundException(
                "No content provider: content://media/external/images/media/1234",
            ),
            SecurityException(
                "Permission Denial: opening provider com.example.vault from " +
                    "ProcessRecord{omnibridge} requires the provider be exported",
            ),
            RuntimeException("/storage/emulated/0/Documents/tax-return-2025.pdf"),
        )
        for (error in leaky) {
            val shown = UiMapping.sendFailureMessage(error)
            assertEquals("Pliwee could not send that file.", shown)
            assertFalse("a URI must not reach the screen", shown.contains("content://"))
            assertFalse("a path must not reach the screen", shown.contains("/storage/"))
            assertFalse(
                "a filename must not reach the screen",
                shown.contains("tax-return-2025"),
            )
        }
    }

    @Test
    fun `an error with no message still says something`() {
        assertEquals(
            "Pliwee could not send that file.",
            UiMapping.sendFailureMessage(IllegalStateException()),
        )
        assertEquals(
            "Pliwee could not send that file.",
            UiMapping.sendFailureMessage(IllegalStateException("   ")),
        )
        assertEquals("Pliwee could not send that file.", UiMapping.sendFailureMessage(null))
    }

    @Test
    fun `the failure message carries nothing but the message`() {
        // A `Failed` holds a string, not a throwable — so there is no field a
        // stack trace or a URI could travel in to a log or a crash reporter.
        val outcome = UiMapping.sendOutcome(
            Result.failure(
                IllegalStateException(
                    "that file could not be read",
                    java.io.FileNotFoundException("content://media/external/1234"),
                ),
            ),
        ) as UiMapping.SendAttempt.Failed

        assertEquals("that file could not be read", outcome.message)
        assertFalse(outcome.toString().contains("content://"))
    }

    @Test
    fun `the button never reads Sending once the attempt has finished`() {
        // The literal symptom of issue #12, as an assertion.
        for (finished in listOf(
            UiMapping.SendAttempt.Idle,
            UiMapping.SendAttempt.Failed("not connected"),
        )) {
            assertNotEquals(
                "a finished attempt must not still read as Sending",
                "Sending…",
                UiMapping.sendButtonLabel(finished),
            )
        }
        assertEquals("Send", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Idle))
        assertEquals("Try again", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Failed("x")))
        assertEquals("Sending…", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Sending))
    }

    @Test
    fun `every offer result produces a button a person can read`() {
        // Label and enablement agree: nothing is both disabled and inviting.
        for (result in listOf(
            Result.success("id"),
            Result.failure(IllegalStateException("not connected")),
            Result.failure(java.io.IOException("opaque")),
        )) {
            val outcome = UiMapping.sendOutcome(result)
            val label = UiMapping.sendButtonLabel(outcome)
            if (outcome.canSend) {
                assertNotEquals("an enabled button must not read as Sending", "Sending…", label)
            }
        }
    }
}
