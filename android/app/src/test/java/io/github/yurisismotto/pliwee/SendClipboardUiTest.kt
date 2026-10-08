package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.proto.Platform
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.ClipboardPreview
import io.github.yurisismotto.pliwee.ui.UiMapping
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Send clipboard screen's decisions, tested without a device.
 *
 * The screen itself is a composition and would need an instrumented run,
 * which uninstalls the app and costs a manual re-pairing. So the decisions
 * live in [UiMapping.sendClipboardUi] and this is what checks them.
 *
 * The one that matters most is the disabled state. This screen was once the
 * most permissive of the three Send clipboard affordances — it asked only
 * whether a link was up — because its condition was written inline in the
 * composable where nothing could reach it.
 */
class SendClipboardUiTest {

    private val peerName = "Fedora"

    private fun peer(
        granted: Boolean = true,
        policy: ClipboardPolicy = ClipboardPolicy(),
    ) = TrustStore.TrustedPeer(
        deviceId = "d0",
        deviceName = peerName,
        fingerprint = Fingerprint.fromHex("ab".repeat(32))!!,
        pairedAtUnix = 0,
        grantedCapabilities = if (granted) setOf(ClipboardCapability.ID) else emptySet(),
        addressHints = emptyList(),
        clipboardPolicy = policy,
    )

    private fun session(peer: TrustStore.TrustedPeer, negotiated: Boolean = true) =
        PliweeApp.LiveSession(
            peerHex = peer.fingerprint.toHex(),
            negotiated = if (negotiated) setOf(ClipboardCapability.ID) else emptySet(),
        )

    private fun ready(peer: TrustStore.TrustedPeer) =
        UiMapping.clipboardSendGate(peer, session(peer))

    private val text = ClipboardPreview(text = "hello", bytes = 5, sensitive = false)
    private val sensitive = ClipboardPreview(text = null, bytes = 32, sensitive = true)

    // ---- the empty state -------------------------------------------------

    @Test
    fun `an empty clipboard disables send and says why`() {
        val peer = peer()
        val ui = UiMapping.sendClipboardUi(ready(peer), preview = null)

        assertFalse("send must be off with nothing to send", ui.enabled)
        assertFalse(ui.hasContent)
        assertEquals(UiMapping.EMPTY_CLIPBOARD_REASON, ui.blockedReason)
    }

    /**
     * The empty-state wording may not imply a retry would help.
     *
     * Android only permits a clipboard read while the app has focus and this
     * screen has already taken its one chance, so "could not read" would be
     * inviting the person to do something that cannot work.
     */
    @Test
    fun `the empty reason does not claim the clipboard could be read again`() {
        val reason = UiMapping.EMPTY_CLIPBOARD_REASON.lowercase()
        listOf("could not read", "try again", "retry", "failed", "permission").forEach {
            assertFalse("empty reason must not say '$it': $reason", reason.contains(it))
        }
    }

    // ---- content present -------------------------------------------------

    @Test
    fun `a clip over a ready session enables send with no reason shown`() {
        val peer = peer()
        val ui = UiMapping.sendClipboardUi(ready(peer), text)

        assertTrue(ui.enabled)
        assertTrue(ui.hasContent)
        assertFalse(ui.contentHidden)
        assertNull("a ready button must not also show a reason", ui.blockedReason)
    }

    /**
     * A sensitive clip is sendable but not shown.
     *
     * Both halves matter: withholding the preview is the privacy rule, and
     * keeping the button live is the truthfulness one — the clip exists, the
     * person may still mean to send it, and greying the button out would say
     * otherwise.
     */
    @Test
    fun `a sensitive clip stays sendable but is withheld from the preview`() {
        val peer = peer()
        val ui = UiMapping.sendClipboardUi(ready(peer), sensitive)

        assertTrue("a sensitive clip is still sendable", ui.enabled)
        assertTrue(ui.hasContent)
        assertTrue("a sensitive clip must not be rendered", ui.contentHidden)
    }

    // ---- every way the send can be blocked -------------------------------

    /**
     * Each blocked gate names the peer and reaches the screen.
     *
     * The peer name is the part that must be dynamic: it is what tells the
     * person which of two paired computers the sentence is about.
     */
    @Test
    fun `every blocked gate is explained and names the peer`() {
        val cases = mapOf(
            "capability not granted" to
                UiMapping.clipboardSendGate(peer(granted = false), session(peer())),
            "sending turned off" to UiMapping.clipboardSendGate(
                peer(policy = ClipboardPolicy(allowSend = false)),
                session(peer()),
            ),
            "no session" to UiMapping.clipboardSendGate(peer(), null),
            "session has not negotiated clipboard" to
                UiMapping.clipboardSendGate(peer(), session(peer(), negotiated = false)),
        )

        cases.forEach { (label, gate) ->
            val ui = UiMapping.sendClipboardUi(gate, text)
            assertFalse("$label must disable send", ui.enabled)
            val reason = ui.blockedReason
            assertNotNull("$label must give a reason", reason)
            reason!!
            assertTrue(
                "$label should name the peer, said: $reason",
                reason.contains(peerName),
            )
        }
    }

    /**
     * A session with a *different* computer must not enable this button.
     *
     * The link state is global — there is one session — so without comparing
     * identities a session with A would light up the Send button aimed at B.
     */
    @Test
    fun `a session with another computer does not enable this peer's send`() {
        val peer = peer()
        val other = PliweeApp.LiveSession(
            peerHex = "cd".repeat(32),
            negotiated = setOf(ClipboardCapability.ID),
        )

        val ui = UiMapping.sendClipboardUi(UiMapping.clipboardSendGate(peer, other), text)

        assertFalse(ui.enabled)
        assertNotNull(ui.blockedReason)
    }

    /**
     * When both the gate and the clipboard are a problem, the gate is named.
     *
     * "Connect to Fedora" is the thing the person has to do first, and it is
     * also the more surprising of the two.
     */
    @Test
    fun `a blocked gate is explained in preference to an empty clipboard`() {
        val ui = UiMapping.sendClipboardUi(UiMapping.clipboardSendGate(peer(), null), null)

        assertFalse(ui.enabled)
        assertTrue(ui.blockedReason!!.contains(peerName))
        assertNotEquals(UiMapping.EMPTY_CLIPBOARD_REASON, ui.blockedReason)
    }

    /** A disabled primary action always says why. Never a dead control. */
    @Test
    fun `a disabled send always carries a reason`() {
        val everyState = listOf(
            UiMapping.sendClipboardUi(ready(peer()), null),
            UiMapping.sendClipboardUi(UiMapping.clipboardSendGate(peer(), null), text),
            UiMapping.sendClipboardUi(UiMapping.clipboardSendGate(peer(), null), null),
            UiMapping.sendClipboardUi(
                UiMapping.clipboardSendGate(peer(granted = false), session(peer())),
                text,
            ),
        )

        everyState.forEach { ui ->
            assertFalse(ui.enabled)
            assertTrue(
                "a disabled send with no explanation is a dead control",
                !ui.blockedReason.isNullOrBlank(),
            )
        }
    }

    // ---- the peer line ---------------------------------------------------

    /**
     * With no session, the line is the pinned fingerprint.
     *
     * Three screens used to print a literal `"Desktop · Linux"` here. The
     * platform is real — it is field 3 of the HELLO — but it arrives with a
     * session, and this app does not store it: a remembered platform is a
     * claim about a machine that is not talking to us.
     */
    @Test
    fun `with no session the peer line is the pinned fingerprint`() {
        val peer = peer()
        val line = UiMapping.peerIdentityLine(peer, session = null)

        assertEquals(peer.fingerprint.toDisplayShort(), line)
        assertFalse("must not invent an operating system", line.contains("Linux"))
        assertFalse("must not invent a form factor", line.contains("Desktop"))
    }

    /** With a live session, the line is what that session's HELLO said. */
    @Test
    fun `a live session shows the platform the peer actually reported`() {
        val peer = peer()
        val linux = PliweeApp.LiveSession(
            peerHex = peer.fingerprint.toHex(),
            negotiated = setOf(ClipboardCapability.ID),
            platform = Platform.PLATFORM_LINUX,
        )
        assertEquals("Desktop · Linux", UiMapping.peerIdentityLine(peer, linux))

        // And an Android peer is not called a desktop, which is exactly what
        // the hardcoded string used to do.
        val android = linux.copy(platform = Platform.PLATFORM_ANDROID)
        val line = UiMapping.peerIdentityLine(peer, android)
        assertTrue(line, line.contains("Android"))
        assertFalse("a phone must not be labelled a desktop", line.contains("Desktop"))
    }

    /**
     * A peer that never stated its platform falls back rather than guessing.
     *
     * `PLATFORM_UNSPECIFIED` is not "Unknown" on screen: it is nothing, and
     * the fingerprint takes the line instead.
     */
    @Test
    fun `an unspecified platform falls back to the fingerprint`() {
        val peer = peer()
        val quiet = PliweeApp.LiveSession(
            peerHex = peer.fingerprint.toHex(),
            negotiated = emptySet(),
            platform = Platform.PLATFORM_UNSPECIFIED,
        )
        assertEquals(peer.fingerprint.toDisplayShort(), UiMapping.peerIdentityLine(peer, quiet))
        assertNull(UiMapping.platformLabel(Platform.PLATFORM_UNSPECIFIED))
    }

    /**
     * A session with another computer must not label this one.
     *
     * The same identity comparison the send gate makes. Without it, one
     * paired desktop's platform would be printed under every device's name.
     */
    @Test
    fun `another computer's session does not label this peer`() {
        val peer = peer()
        val other = PliweeApp.LiveSession(
            peerHex = "cd".repeat(32),
            negotiated = setOf(ClipboardCapability.ID),
            platform = Platform.PLATFORM_LINUX,
        )
        assertEquals(peer.fingerprint.toDisplayShort(), UiMapping.peerIdentityLine(peer, other))
    }

    /** Two computers with the same name are still told apart. */
    @Test
    fun `the peer line distinguishes two devices with the same name`() {
        val a = peer()
        val b = a.copy(fingerprint = Fingerprint.fromHex("cd".repeat(32))!!)

        assertEquals(a.deviceName, b.deviceName)
        assertNotEquals(
            UiMapping.peerIdentityLine(a, null),
            UiMapping.peerIdentityLine(b, null),
        )
    }
}
