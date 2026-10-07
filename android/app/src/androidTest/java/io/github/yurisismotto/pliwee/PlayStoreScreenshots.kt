package io.github.yurisismotto.pliwee

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.capability.NotificationsCapability
import io.github.yurisismotto.pliwee.files.FileTransferManager
import io.github.yurisismotto.pliwee.files.TransferState
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.proto.Platform
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.ClipboardPreview
import io.github.yurisismotto.pliwee.ui.MainUiState
import io.github.yurisismotto.pliwee.ui.PliweeShell
import io.github.yurisismotto.pliwee.ui.theme.PliweeTheme
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Google Play listing screenshots, from the real shell and the real screens.
 *
 * Not a test of anything, and inert unless asked for: every case is skipped
 * unless the instrumentation argument `playScreenshots=true` is passed, which
 * only `android/tools/capture-play-screenshots.sh` does. Nothing here is in
 * the release build — it is `androidTest` source, and the activity hosting it
 * is the debug-only `ComponentActivity` from `ui-test-manifest`.
 *
 * The state is fictional and contains nothing from this device or any real
 * computer: a "Linux Desktop" whose fingerprint is a made-up constant, no
 * addresses, and a clipboard preview of "Hello from Pliwee". Navigation is by
 * tapping, the same way a person reaches each screen, so what is captured is
 * what [PliweeShell] draws for that state — not a screen composed out of
 * context.
 *
 * Each capture is `screencap` run as the shell user into [OUT_DIR], which the
 * script pulls from. The file is checked to exist and be non-empty here,
 * so a capture that did not happen fails rather than leaving a stale PNG.
 */
@RunWith(AndroidJUnit4::class)
class PlayStoreScreenshots {

    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun onlyWhenAsked() {
        val asked = InstrumentationRegistry.getArguments().getString("playScreenshots")
        assumeTrue("Play screenshots run only when asked for", asked == "true")
    }

    @Test
    fun capture() {
        compose.setContent {
            PliweeTheme {
                Surface(Modifier.fillMaxSize(), color = PliweeTheme.colors.background) {
                    PliweeShell(state = state(), actions = actions())
                }
            }
        }

        settle()
        shoot("01-devices")

        compose.onNodeWithText(PEER_NAME).performClick()
        settle()
        showAllPermissions()
        shoot("02-device-details")

        // "Send clipboard" is both a section switch and the button; the button
        // is the clickable one further down, reached by scrolling.
        compose.onAllNodesWithText("Send clipboard").let { nodes ->
            nodes[nodes.fetchSemanticsNodes().size - 1].performScrollTo().performClick()
        }
        settle()
        shoot("03-clipboard")

        compose.onNodeWithContentDescription("Files").performClick()
        settle()
        shoot("04-files-or-notifications")
    }

    /**
     * Brings the fourth permission, Notifications, on screen.
     *
     * On a tall display it already is, and nothing scrolls. Otherwise the
     * screen is scrolled to start at the computer's name, so the header icon
     * is not left cut in half under the title bar.
     */
    private fun showAllPermissions() {
        val scroller = compose.onNode(hasScrollAction())
        val viewport = scroller.fetchSemanticsNode().boundsInRoot
        val entry = compose.onNodeWithContentDescription("Notifications")
        // Unclipped, from position and size. `boundsInRoot` is clipped to the
        // scroll viewport and is an empty rect for a node wholly outside it:
        // measured, a check against it read "on screen" for an entry that was
        // out of view, and passed over a capture without it.
        fun bottom() = entry.fetchSemanticsNode().let { it.positionInRoot.y + it.size.height }
        assertTrue("Notifications entry has no size", entry.fetchSemanticsNode().size.height > 0)
        if (bottom() > viewport.bottom) {
            val title = compose.onNodeWithText(PEER_NAME).fetchSemanticsNode().positionInRoot
            scroller.performSemanticsAction(SemanticsActions.ScrollBy) {
                it(0f, title.y - viewport.top)
            }
            settle()
        }
        val node = entry.fetchSemanticsNode()
        assertTrue(
            "Notifications is not fully on screen",
            bottom() <= viewport.bottom && node.boundsInRoot.height == node.size.height.toFloat(),
        )
    }

    /** Lets the crossfade and any press ripple finish before a capture. */
    private fun settle() {
        compose.waitForIdle()
        Thread.sleep(SETTLE_MS)
        compose.waitForIdle()
    }

    private fun shoot(name: String) {
        val path = "$OUT_DIR/$name.png"
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        shell(automation, "mkdir -p $OUT_DIR")
        shell(automation, "rm -f $path")
        shell(automation, "screencap -p $path")
        val size = shell(automation, "stat -c %s $path").trim().toLongOrNull() ?: 0L
        assertTrue("screencap wrote nothing to $path", size > 0L)
    }

    /** Runs [command] as the shell user and waits for it, returning stdout. */
    private fun shell(automation: android.app.UiAutomation, command: String): String =
        automation.executeShellCommand(command).let { pfd ->
            android.os.ParcelFileDescriptor.AutoCloseInputStream(pfd).use {
                it.readBytes().decodeToString()
            }
        }

    private val peer = TrustStore.TrustedPeer(
        deviceId = "linux-desktop",
        deviceName = PEER_NAME,
        fingerprint = Fingerprint.fromHex(PEER_HEX)!!,
        pairedAtUnix = 1_790_000_000,
        grantedCapabilities = setOf(
            ClipboardCapability.ID,
            FilesCapability.ID,
            BatteryCapability.ID,
            NotificationsCapability.ID,
        ),
        addresses = emptyList(),
        notificationPolicy = NotificationPolicy(
            allowMirror = true,
            allowedApps = setOf("com.example.calendar", "com.example.messages"),
        ),
    )

    private fun state(): MainUiState {
        val base = NotificationUiFixtures.state(
            peer = peer,
            status = NotificationUiFixtures.sourcingStatus(),
        )
        return base.copy(
            ownDeviceName = "Pixel",
            connection = PliweeApp.ConnectionState.Connected(
                PEER_NAME,
                peer.fingerprint.toDisplayShort(),
            ),
            selectedPeerHex = PEER_HEX,
            notifications = NotificationUiFixtures.sourcingStatus().copy(
                peers = NotificationUiFixtures.sourcingStatus().peers.mapKeys { PEER_HEX },
            ),
            liveSession = PliweeApp.LiveSession(
                peerHex = PEER_HEX,
                negotiated = peer.grantedCapabilities,
                platform = Platform.PLATFORM_LINUX,
            ),
            remoteBatteryPercent = 82,
            transfers = listOf(
                FileTransferManager.TransferUi(
                    transferId = "demo-2",
                    peer = peer.fingerprint,
                    filename = "notes.pdf",
                    sizeBytes = 1_300_000,
                    bytesTransferred = 820_000,
                    sending = true,
                    state = TransferState.TRANSFERRING,
                    failure = null,
                    mimeType = "application/pdf",
                    sequence = 2,
                ),
                FileTransferManager.TransferUi(
                    transferId = "demo-1",
                    peer = peer.fingerprint,
                    filename = "photo.jpg",
                    // 2.4 MiB, which the screen formats as "2.4 MB".
                    sizeBytes = 2_516_582,
                    bytesTransferred = 2_516_582,
                    sending = false,
                    state = TransferState.COMPLETED,
                    failure = null,
                    mimeType = "image/jpeg",
                    hasOpenTarget = true,
                    sequence = 1,
                    settledSequence = 3,
                ),
            ),
        )
    }

    private fun actions() = NotificationUiFixtures.Recorder().actions().copy(
        readClipboardPreview = {
            ClipboardPreview(text = CLIP, bytes = CLIP.encodeToByteArray().size, sensitive = false)
        },
    )

    private companion object {
        const val PEER_NAME = "Linux Desktop"

        /** Made up. Not the key of any real computer. */
        const val PEER_HEX = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90"
        const val CLIP = "Hello from Pliwee"
        const val OUT_DIR = "/data/local/tmp/pliwee-play-screenshots"
        const val SETTLE_MS = 1_500L
    }
}
