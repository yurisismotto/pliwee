package io.github.yurisismotto.pliwee

import com.google.protobuf.ByteString
import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardDelivery
import io.github.yurisismotto.pliwee.clipboard.ClipboardLimits
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.clipboard.ClipboardSendFailed
import io.github.yurisismotto.pliwee.clipboard.ClipboardSync
import io.github.yurisismotto.pliwee.clipboard.ClipboardTarget
import io.github.yurisismotto.pliwee.clipboard.ClipboardText
import io.github.yurisismotto.pliwee.clipboard.Clock
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.proto.ErrorCode
import io.github.yurisismotto.pliwee.proto.capabilities.ClipboardControl
import io.github.yurisismotto.pliwee.proto.capabilities.ClipboardOutcome
import io.github.yurisismotto.pliwee.proto.capabilities.ClipboardResult
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import java.security.SecureRandom
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GitHub #8 — a clipboard send says only what is actually known.
 *
 * Two halves, and they fail for unrelated reasons, so they are asserted
 * separately:
 *
 *  * the **gate** ([UiMapping.clipboardSendGate]) decides whether a send may
 *    start, from the grant, the per-peer policy, and what the live session
 *    negotiated;
 *  * the **verdict** ([ClipboardSync]) decides what to say afterwards, from
 *    the peer's own answer correlated to the frame that caused it.
 *
 * Neither is allowed to stand in for the other. A passing gate does not make
 * a send confirmed, and that is the defect: `sendText` used to answer
 * `Result.success(bytes)` as soon as the frame was written, and the person
 * was told "Sent 42 bytes" over a clip the desktop had dropped.
 *
 * The letters map to the brief's §17 list.
 */
class ClipboardTruthfulnessTest {

    // -----------------------------------------------------------------------
    // Harness
    // -----------------------------------------------------------------------

    private class FakeClipboard : ClipboardTarget {
        var current: ClipboardTarget.ReadClip? = null
        override fun read(): Result<ClipboardTarget.ReadClip> {
            val clip = current ?: return Result.failure(Unreadable())
            return Result.success(clip)
        }

        override fun write(
            text: ClipboardText,
            sensitive: Boolean,
            label: String,
        ): ClipboardTarget.WriteResult = ClipboardTarget.WriteResult.Applied

        fun userCopies(text: String, sensitive: Boolean = false) {
            current = ClipboardTarget.ReadClip(ClipboardText.validate(text).getOrThrow(), sensitive)
        }
    }

    private class Unreadable : Exception("empty")

    private class FakeAuthorizer : ClipboardSync.Authorizer {
        private val policies = mutableMapOf<String, ClipboardPolicy>()
        fun grant(peer: Fingerprint, policy: ClipboardPolicy = ClipboardPolicy()) {
            policies[peer.toHex()] = policy
        }

        fun revoke(peer: Fingerprint) {
            policies.remove(peer.toHex())
        }

        override fun policyFor(peer: Fingerprint): ClipboardPolicy =
            policies[peer.toHex()] ?: ClipboardPolicy.DENIED
    }

    /** One fake session, recording what it was asked to write. */
    private class Session {
        val frames = mutableListOf<ByteString>()
        val messageIds = mutableListOf<ByteString>()
    }

    private class Rig {
        val clipboard = FakeClipboard()
        val authorizer = FakeAuthorizer()
        val sessions = mutableMapOf<String, Session>()
        val sync = ClipboardSync(
            systemClipboard = clipboard,
            authorizer = authorizer,
            localDeviceId = "phone-device-id",
            clock = Clock { 0 },
            random = SecureRandom(),
        )

        suspend fun connect(peer: Fingerprint): Session {
            val session = Session()
            sessions[peer.toHex()] = session
            sync.attachSession(peer) { payload ->
                session.frames += payload
                val id = ByteString.copyFrom(ByteArray(16).also { SecureRandom().nextBytes(it) })
                session.messageIds += id
                id
            }
            return session
        }

        suspend fun disconnect(peer: Fingerprint) {
            sessions.remove(peer.toHex())
            sync.detachSession(peer)
        }

        /** The event id of the n-th frame this session sent. */
        fun eventIdOf(session: Session, index: Int = 0): ByteString =
            ClipboardControl.parseFrom(session.frames[index]).update.eventId

        /** Feeds a peer's `ClipboardResult` in, as the capability would. */
        suspend fun verdict(peer: Fingerprint, eventId: ByteString, outcome: ClipboardOutcome) {
            sync.handleControl(
                peer,
                "desktop",
                "Fedora",
                ClipboardControl.newBuilder()
                    .setResult(
                        ClipboardResult.newBuilder().setEventId(eventId).setOutcome(outcome),
                    )
                    .build()
                    .toByteString(),
            )
        }
    }

    private fun fp(byte: Byte) = Fingerprint(ByteArray(32) { byte })

    private fun peer(
        fingerprintByte: Byte = 0,
        vararg capabilities: String,
        name: String = "Fedora",
        policy: ClipboardPolicy = ClipboardPolicy(),
    ) = TrustStore.TrustedPeer(
        deviceId = "d$fingerprintByte",
        deviceName = name,
        fingerprint = fp(fingerprintByte),
        pairedAtUnix = 0,
        grantedCapabilities = capabilities.toSet(),
        addressHints = emptyList(),
        clipboardPolicy = policy,
    )

    private fun live(fingerprintByte: Byte = 0, vararg negotiated: String) =
        PliweeApp.LiveSession(
            peerHex = fp(fingerprintByte).toHex(),
            negotiated = negotiated.toSet(),
        )

    // =======================================================================
    // The gate — may a send start at all?
    // =======================================================================

    /**
     * **C1 — grant, policy and a live session, but the session never
     * negotiated `clipboard.v1`: blocked.**
     *
     * The state the old UI allowed. All three of the conditions it checked
     * were true, so the button was live; the send then failed inside
     * `sendText` with "Not connected to that computer", which was false —
     * the session was up — and told the person nothing they could act on.
     */
    @Test
    fun c1_a_session_without_the_capability_blocks_the_send() {
        val gate = UiMapping.clipboardSendGate(
            peer(1, ClipboardCapability.ID),
            live(1, BatteryCapability.ID, FilesCapability.ID),
        )
        assertFalse(gate.ready)
        assertTrue(
            "the reason must name the negotiation, not the connection: $gate",
            gate.reasonOrNull!!.contains("has not negotiated clipboard sharing"),
        )
    }

    /** **C2 — with `clipboard.v1` negotiated, the send is allowed.** */
    @Test
    fun c2_a_negotiated_session_allows_the_send() {
        assertTrue(
            UiMapping.clipboardSendGate(
                peer(2, ClipboardCapability.ID),
                live(2, ClipboardCapability.ID),
            ).ready,
        )
    }

    /**
     * **C3 — a grant revoked mid-session blocks immediately.**
     *
     * No reconnect, no handshake, no waiting. The gate re-reads the trust
     * store record it is handed, and the send path re-asks the authorizer, so
     * withdrawal bites on the session that is already up.
     */
    @Test
    fun c3_revoking_the_grant_blocks_at_once() = runTest {
        // The UI half.
        val session = live(3, ClipboardCapability.ID)
        assertTrue(UiMapping.clipboardSendGate(peer(3, ClipboardCapability.ID), session).ready)
        assertFalse(
            "the same live session must not authorise an ungranted peer",
            UiMapping.clipboardSendGate(peer(3), session).ready,
        )

        // And the send half, on the same live session.
        val rig = Rig()
        val target = fp(3)
        rig.authorizer.grant(target)
        rig.connect(target)
        rig.clipboard.userCopies("still fine")
        assertTrue(rig.sync.sendCurrentClipboard(target).isSuccess)

        rig.authorizer.revoke(target)
        val refused = rig.sync.sendCurrentClipboard(target)
        assertTrue(refused.isFailure)
        assertEquals(
            ClipboardSync.SendFailure.NotPermitted,
            (refused.exceptionOrNull() as ClipboardSendFailed).failure,
        )
    }

    /** **C4 — the send direction being off blocks regardless of negotiation.** */
    @Test
    fun c4_policy_denied_blocks_whatever_the_session_negotiated() {
        val gate = UiMapping.clipboardSendGate(
            peer(4, ClipboardCapability.ID, policy = ClipboardPolicy(allowSend = false)),
            live(4, ClipboardCapability.ID),
        )
        assertFalse(gate.ready)
        assertTrue(gate.reasonOrNull!!.contains("is turned off"))
    }

    /**
     * **C5 — another computer's session cannot authorise this one.**
     *
     * With two desktops paired there is one link state and two peers. A
     * boolean "connected" would let the session with A enable the Send button
     * aimed at B, which is the same class of defect as U2 §39.17 — a global
     * fact standing in for a per-identity one.
     */
    @Test
    fun c5_a_session_with_another_peer_does_not_authorise_this_one() {
        val gate = UiMapping.clipboardSendGate(
            peer(5, ClipboardCapability.ID),
            // Fully negotiated — with somebody else.
            live(6, ClipboardCapability.ID),
        )
        assertFalse(gate.ready)
        assertTrue(gate.reasonOrNull!!.contains("Connect to"))
    }

    /** **C6 — no session at all blocks the send.** */
    @Test
    fun c6_no_session_blocks_the_send() {
        val gate = UiMapping.clipboardSendGate(peer(7, ClipboardCapability.ID), null)
        assertFalse(gate.ready)
        assertTrue(gate.reasonOrNull!!.contains("Connect to"))
    }

    /**
     * **C18 — nothing here invents a negotiated capability.**
     *
     * The gate is a reflection, never a widening. Granting `clipboard.v1` in
     * the trust store does not put it on a session that did not negotiate it,
     * and no combination of grants can.
     */
    @Test
    fun c18_the_gate_never_widens_the_negotiated_set() {
        val everything = peer(
            8,
            ClipboardCapability.ID,
            FilesCapability.ID,
            BatteryCapability.ID,
        )
        for (negotiated in listOf(
            live(8),
            live(8, BatteryCapability.ID),
            live(8, FilesCapability.ID, BatteryCapability.ID),
        )) {
            assertFalse(
                "a grant must never put a capability on a session",
                UiMapping.clipboardSendGate(everything, negotiated).ready,
            )
        }
    }

    // =======================================================================
    // The verdict — what may be said afterwards?
    // =======================================================================

    /**
     * **C10 — writing the frame is never a confirmed success.**
     *
     * The heart of #8. `sendCurrentClipboard` succeeding means the frame is
     * on the session and that is all it has ever meant.
     */
    @Test
    fun c10_a_local_write_alone_is_not_confirmed() = runTest {
        val rig = Rig()
        val target = fp(10)
        rig.authorizer.grant(target)
        val session = rig.connect(target)
        rig.clipboard.userCopies("a password")

        val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
        assertEquals(1, session.frames.size)

        val known = rig.sync.lastDelivery.value[target.toHex()]
        assertEquals(ClipboardDelivery.Enqueued(10), known)
        assertFalse("an enqueued frame is not a success", known!!.succeeded)
        assertFalse("and it is not settled either", known.settled)
        assertFalse(
            "the wording must not claim delivery",
            known.describe("Fedora").contains("received"),
        )
        assertTrue(known.describe("Fedora").contains("not confirmed it yet"))
        assertEquals(1, rig.sync.outstandingSends())

        // Still not a success after the wait gives up.
        val timedOut = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        assertFalse(timedOut.succeeded)
        assertEquals(
            ClipboardDelivery.Unconfirmed(
                ClipboardDelivery.Unconfirmed.Reason.TIMED_OUT,
                10,
            ),
            timedOut,
        )
        assertTrue(timedOut.describe("Fedora").contains("delivery not confirmed"))
    }

    /** **C9 — a confirmed peer result is a confirmed success.** */
    @Test
    fun c9_an_applied_result_confirms_the_send() = runTest {
        val rig = Rig()
        val target = fp(11)
        rig.authorizer.grant(target)
        val session = rig.connect(target)
        rig.clipboard.userCopies("hello")

        val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
        rig.verdict(target, rig.eventIdOf(session), ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED)

        val delivery = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        assertEquals(ClipboardDelivery.Confirmed(ClipboardSync.Outcome.APPLIED, 5), delivery)
        assertTrue(delivery.succeeded)
        assertEquals(delivery, rig.sync.lastDelivery.value[target.toHex()])
        assertEquals("nothing may be left outstanding", 0, rig.sync.outstandingSends())

        // `PENDING_USER` is a success too: the clip arrived, and the person at
        // the desktop decides when to apply it. Reporting it as a failure
        // would be as untrue as the defect being fixed.
        rig.clipboard.userCopies("held")
        val second = rig.sync.sendCurrentClipboard(target).getOrThrow()
        rig.verdict(
            target,
            rig.eventIdOf(session, 1),
            ClipboardOutcome.CLIPBOARD_OUTCOME_PENDING_USER,
        )
        assertTrue(second.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
    }

    /**
     * **C8 — a not-authorized result is a visible rejection.**
     *
     * Every refusing outcome, so that one added later cannot default to
     * looking like success.
     */
    @Test
    fun c8_every_refusing_outcome_reads_as_a_rejection() = runTest {
        val refusals = listOf(
            ClipboardOutcome.CLIPBOARD_OUTCOME_NOT_AUTHORIZED to
                ClipboardSync.Outcome.NOT_AUTHORIZED,
            ClipboardOutcome.CLIPBOARD_OUTCOME_REJECTED_POLICY to
                ClipboardSync.Outcome.REJECTED_POLICY,
            ClipboardOutcome.CLIPBOARD_OUTCOME_REJECTED_SENSITIVE to
                ClipboardSync.Outcome.REJECTED_SENSITIVE,
            ClipboardOutcome.CLIPBOARD_OUTCOME_TOO_LARGE to ClipboardSync.Outcome.TOO_LARGE,
            ClipboardOutcome.CLIPBOARD_OUTCOME_INVALID_TEXT to
                ClipboardSync.Outcome.INVALID_TEXT,
            ClipboardOutcome.CLIPBOARD_OUTCOME_FAILED to ClipboardSync.Outcome.FAILED,
        )
        for ((proto, expected) in refusals) {
            val rig = Rig()
            val target = fp(12)
            rig.authorizer.grant(target)
            val session = rig.connect(target)
            rig.clipboard.userCopies("x")

            val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
            rig.verdict(target, rig.eventIdOf(session), proto)

            val delivery = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
            assertEquals(ClipboardDelivery.Rejected(expected, 1), delivery)
            assertFalse("$proto must never read as success", delivery.succeeded)
            assertTrue(delivery.settled)
        }
    }

    /**
     * **C7 — an unsupported-capability refusal reaches the person.**
     *
     * The path GitHub #8 names. A desktop whose session never negotiated
     * `clipboard.v1` cannot answer in `clipboard.v1`; it answers with a
     * transport `Error` whose `correlation_id` is the `message_id` of the
     * frame it refused. That id was minted here, so the refusal is
     * attributable — which is what turns a log line into a message.
     */
    @Test
    fun c7_an_unsupported_capability_error_rejects_the_send() = runTest {
        val rig = Rig()
        val target = fp(13)
        rig.authorizer.grant(target)
        val session = rig.connect(target)
        rig.clipboard.userCopies("secret")

        val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
        rig.sync.onPeerRefusal(
            target,
            session.messageIds.single(),
            ErrorCode.ERROR_CODE_UNSUPPORTED_CAPABILITY,
        )

        val delivery = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        assertEquals(
            ClipboardDelivery.Refused(ClipboardDelivery.Refusal.NOT_NEGOTIATED, 6),
            delivery,
        )
        assertFalse(delivery.succeeded)
        val message = delivery.describe("Fedora")
        assertTrue(message.contains("has not negotiated clipboard sharing"))
        assertTrue("it must say nothing left", message.contains("Nothing was sent"))
    }

    /** The other refusal codes map to their own words, and none to success. */
    @Test
    fun peer_error_codes_map_to_a_closed_vocabulary() = runTest {
        val cases = mapOf(
            ErrorCode.ERROR_CODE_NOT_AUTHORIZED to ClipboardDelivery.Refusal.NOT_AUTHORIZED,
            ErrorCode.ERROR_CODE_RATE_LIMITED to ClipboardDelivery.Refusal.RATE_LIMITED,
            ErrorCode.ERROR_CODE_INTERNAL to ClipboardDelivery.Refusal.OTHER,
            ErrorCode.ERROR_CODE_MALFORMED to ClipboardDelivery.Refusal.OTHER,
        )
        for ((code, expected) in cases) {
            val rig = Rig()
            val target = fp(14)
            rig.authorizer.grant(target)
            val session = rig.connect(target)
            rig.clipboard.userCopies("x")

            val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
            rig.sync.onPeerRefusal(target, session.messageIds.single(), code)
            val delivery = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
            assertEquals(ClipboardDelivery.Refused(expected, 1), delivery)
            assertFalse(delivery.succeeded)
        }
    }

    /**
     * **C11 — a pending send is settled honestly when the session ends.**
     *
     * Not dropped and not left hanging. A caller waiting for a verdict is
     * answered, and the state the screen draws stops saying "waiting" over a
     * session that is gone.
     */
    @Test
    fun c11_disconnecting_settles_a_pending_send_as_unconfirmed() = runTest {
        val rig = Rig()
        val target = fp(15)
        rig.authorizer.grant(target)
        rig.connect(target)
        rig.clipboard.userCopies("in flight")

        val receipt = rig.sync.sendCurrentClipboard(target).getOrThrow()
        rig.disconnect(target)

        val delivery = receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        assertEquals(
            ClipboardDelivery.Unconfirmed(
                ClipboardDelivery.Unconfirmed.Reason.DISCONNECTED,
                9,
            ),
            delivery,
        )
        assertFalse(delivery.succeeded)
        assertEquals(delivery, rig.sync.lastDelivery.value[target.toHex()])
        assertEquals(0, rig.sync.outstandingSends())
        assertTrue(delivery.describe("Fedora").contains("not confirmed"))
    }

    /**
     * **C12 — a verdict from a previous session cannot resolve a new send.**
     *
     * The session ends, a new one comes up, and the old computer's answer
     * arrives late. It names an event id that belonged to a send the teardown
     * already settled, so it resolves nothing — and emphatically not the clip
     * that is in flight now.
     */
    @Test
    fun c12_a_stale_verdict_cannot_resolve_a_new_send() = runTest {
        val rig = Rig()
        val target = fp(16)
        rig.authorizer.grant(target)
        val first = rig.connect(target)
        rig.clipboard.userCopies("old clip")
        rig.sync.sendCurrentClipboard(target).getOrThrow()
        val staleEventId = rig.eventIdOf(first)

        rig.disconnect(target)
        val second = rig.connect(target)
        rig.clipboard.userCopies("new clip")
        val fresh = rig.sync.sendCurrentClipboard(target).getOrThrow()

        // The previous session's verdict, arriving now.
        rig.verdict(target, staleEventId, ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED)
        assertEquals(
            "the new send must still be outstanding",
            1,
            rig.sync.outstandingSends(),
        )
        assertEquals(
            ClipboardDelivery.Enqueued(8),
            rig.sync.lastDelivery.value[target.toHex()],
        )

        // And the current session's own verdict still lands.
        rig.verdict(
            target,
            rig.eventIdOf(second),
            ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED,
        )
        assertTrue(fresh.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
    }

    /**
     * **C13 — a result for A cannot resolve a send to B.**
     *
     * Both directions: B answering with A's event id resolves nothing, and B
     * answering with an id of its own resolves only its own.
     */
    @Test
    fun c13_a_result_from_one_peer_cannot_resolve_another_peers_send() = runTest {
        val rig = Rig()
        val a = fp(17)
        val b = fp(18)
        rig.authorizer.grant(a)
        rig.authorizer.grant(b)
        val sessionA = rig.connect(a)
        val sessionB = rig.connect(b)

        rig.clipboard.userCopies("for A")
        val toA = rig.sync.sendCurrentClipboard(a).getOrThrow()
        rig.clipboard.userCopies("for B!")
        val toB = rig.sync.sendCurrentClipboard(b).getOrThrow()

        // The two clips differ in length on purpose, so a state that had been
        // attributed to the wrong peer would be visible as the wrong number.
        // B claims A's event. Nothing may move.
        rig.verdict(b, rig.eventIdOf(sessionA), ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED)
        assertEquals(ClipboardDelivery.Enqueued(5), rig.sync.lastDelivery.value[a.toHex()])
        assertEquals(ClipboardDelivery.Enqueued(6), rig.sync.lastDelivery.value[b.toHex()])
        assertEquals(2, rig.sync.outstandingSends())

        // And a refusal from B correlated to A's *frame* is refused too.
        rig.sync.onPeerRefusal(
            b,
            sessionA.messageIds.single(),
            ErrorCode.ERROR_CODE_UNSUPPORTED_CAPABILITY,
        )
        assertEquals(ClipboardDelivery.Enqueued(5), rig.sync.lastDelivery.value[a.toHex()])

        // Each peer's own answer resolves its own send, and only that one.
        rig.verdict(b, rig.eventIdOf(sessionB), ClipboardOutcome.CLIPBOARD_OUTCOME_NOT_AUTHORIZED)
        assertFalse(toB.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
        rig.verdict(a, rig.eventIdOf(sessionA), ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED)
        assertTrue(toA.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
    }

    /**
     * Several sends may be outstanding at once, and each is resolved by its
     * own result.
     *
     * Not a single "waiting" flag: nothing in `clipboard_v1.proto` says a
     * sender must wait for a verdict before sending again, so the state is
     * keyed by the protocol's identifier rather than by the fact that
     * *something* is in flight. Resolved out of order on purpose.
     */
    @Test
    fun concurrent_sends_are_each_resolved_by_their_own_result() = runTest {
        val rig = Rig()
        val target = fp(19)
        rig.authorizer.grant(target)
        val session = rig.connect(target)

        val receipts = (0 until 4).map { i ->
            rig.clipboard.userCopies("clip $i")
            rig.sync.sendCurrentClipboard(target).getOrThrow()
        }
        assertEquals(4, rig.sync.outstandingSends())
        assertEquals(4, receipts.map { it.eventIdHex }.toSet().size)

        rig.verdict(target, rig.eventIdOf(session, 2), ClipboardOutcome.CLIPBOARD_OUTCOME_APPLIED)
        rig.verdict(
            target,
            rig.eventIdOf(session, 0),
            ClipboardOutcome.CLIPBOARD_OUTCOME_TOO_LARGE,
        )

        assertTrue(receipts[2].awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
        assertFalse(receipts[0].awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS).succeeded)
        assertEquals("the other two are still in flight", 2, rig.sync.outstandingSends())
    }

    /**
     * **C14 — no clipboard content reaches the outcome model or its wording.**
     *
     * Asserted over a canary that would be unmistakable if it leaked, through
     * every state the model has. `ClipboardDelivery` has no `String` field a
     * clip could reach, so this is a check on the *wording* — the part a
     * person sees and an accessibility service reads aloud.
     */
    @Test
    fun c14_no_clipboard_text_appears_in_any_outcome_or_its_wording() = runTest {
        val canary = "CANARY-hunter2-swordfish"
        val rig = Rig()
        val target = fp(20)
        rig.authorizer.grant(target)
        val session = rig.connect(target)
        rig.clipboard.userCopies(canary, sensitive = true)

        val receipt = rig.sync.sendCurrentClipboard(target, confirmedSensitive = true).getOrThrow()

        val states = mutableListOf<ClipboardDelivery>(receipt.enqueued)
        rig.verdict(
            target,
            rig.eventIdOf(session),
            ClipboardOutcome.CLIPBOARD_OUTCOME_REJECTED_SENSITIVE,
        )
        states += receipt.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        states += rig.sync.lastDelivery.value.values

        // And the states nothing on this path produces, stated exhaustively
        // so a new one cannot be added with a message that interpolates text.
        states += ClipboardSync.Outcome.entries.map { ClipboardDelivery.of(it, canary.length) }
        states += ClipboardDelivery.Refusal.entries.map {
            ClipboardDelivery.Refused(it, canary.length)
        }
        states += ClipboardDelivery.Unconfirmed.Reason.entries.map {
            ClipboardDelivery.Unconfirmed(it, canary.length)
        }

        for (state in states) {
            val rendered = "$state ${state.describe("Fedora")}"
            assertFalse("clipboard text leaked into $state", rendered.contains("CANARY"))
            assertFalse(rendered.contains("hunter2"))
            assertFalse(rendered.contains("swordfish"))
        }
    }

    /**
     * **C15 — the sensitive rule is unchanged and still fail-closed.**
     *
     * A clip the platform marked sensitive does not leave on one tap, the
     * refusal carries no text, and nothing about the new verdict model gives
     * it a second route out.
     */
    @Test
    fun c15_a_sensitive_clip_still_needs_confirmation_and_leaks_nothing() = runTest {
        val rig = Rig()
        val target = fp(21)
        rig.authorizer.grant(target)
        val session = rig.connect(target)
        rig.clipboard.userCopies("hunter2", sensitive = true)

        val refused = rig.sync.sendCurrentClipboard(target, confirmedSensitive = false)
        val failure = (refused.exceptionOrNull() as ClipboardSendFailed).failure
        assertTrue(failure is ClipboardSync.SendFailure.NeedsConfirmation)
        assertTrue("nothing may have left", session.frames.isEmpty())
        assertFalse(failure.describe().contains("hunter2"))
        assertNull(
            "and nothing may be recorded about a send that did not happen",
            rig.sync.lastDelivery.value[target.toHex()],
        )

        // A peer that refuses sensitive clips is a rejection, not a success.
        rig.sync.sendCurrentClipboard(target, confirmedSensitive = true).getOrThrow()
        rig.verdict(
            target,
            rig.eventIdOf(session),
            ClipboardOutcome.CLIPBOARD_OUTCOME_REJECTED_SENSITIVE,
        )
        val delivery = rig.sync.lastDelivery.value[target.toHex()]!!
        assertFalse(delivery.succeeded)
        assertTrue(delivery.describe("Fedora").contains("sensitive"))
    }

    /**
     * **C16 / C17 — grant convergence, in both directions.**
     *
     * Android's `HELLO` advertises what this build *implements*, not what any
     * peer has been granted ([CapabilityRegistry.advertised]), so an
     * Android-side grant is not an input to negotiation and cannot make a
     * negotiated set stale. Both directions therefore converge without a
     * handshake, and both are checked here rather than assumed:
     *
     *  * **widening** takes effect on the session that is already up, because
     *    the authorizer is re-asked per send;
     *  * **narrowing** stops sends on that same session, immediately.
     *
     * This is the honest answer to the brief's §9, and it is why no
     * clipboard-specific reconnect was added: there is nothing for one to
     * converge. The *desktop* side is the one whose advertisement is
     * grant-filtered, and it already has `renegotiate_after_grant`.
     */
    @Test
    fun c16_c17_grant_changes_converge_on_the_live_session() = runTest {
        val rig = Rig()
        val target = fp(22)
        val session = rig.connect(target)
        rig.clipboard.userCopies("text")

        // Not granted: refused, on a session that is fully up.
        assertTrue(rig.sync.sendCurrentClipboard(target).isFailure)
        assertTrue(session.frames.isEmpty())

        // C16 — widening takes effect at once, on this same session.
        rig.authorizer.grant(target)
        assertTrue(rig.sync.sendCurrentClipboard(target).isSuccess)
        assertEquals(1, session.frames.size)

        // C17 — narrowing stops it at once, on this same session.
        rig.authorizer.revoke(target)
        assertTrue(rig.sync.sendCurrentClipboard(target).isFailure)
        assertEquals("nothing further may have left", 1, session.frames.size)

        // And the direction alone is enough, without touching the grant.
        rig.authorizer.grant(target, ClipboardPolicy(allowSend = false))
        assertTrue(rig.sync.sendCurrentClipboard(target).isFailure)
        assertEquals(1, session.frames.size)
    }

    /**
     * Outstanding sends are bounded, and eviction settles rather than drops.
     *
     * A peer that answers nothing must not be able to grow this process's
     * memory, and a caller waiting on an evicted send must still be answered.
     */
    @Test
    fun outstanding_sends_are_bounded_and_eviction_is_honest() = runTest {
        val rig = Rig()
        val target = fp(23)
        rig.authorizer.grant(target)
        rig.connect(target)

        val first = run {
            rig.clipboard.userCopies("the first one")
            rig.sync.sendCurrentClipboard(target).getOrThrow()
        }
        repeat(64) { i ->
            rig.clipboard.userCopies("clip $i")
            rig.sync.sendCurrentClipboard(target).getOrThrow()
        }
        assertTrue("the map must stay bounded", rig.sync.outstandingSends() <= 32)

        val evicted = first.awaitVerdict(ClipboardLimits.VERDICT_TIMEOUT_MS)
        assertFalse("an evicted send is never a success", evicted.succeeded)
        assertTrue(evicted is ClipboardDelivery.Unconfirmed)
    }
}
