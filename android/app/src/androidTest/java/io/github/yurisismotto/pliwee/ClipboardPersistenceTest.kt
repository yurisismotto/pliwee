package io.github.yurisismotto.pliwee

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.clipboard.ClipboardSync
import io.github.yurisismotto.pliwee.clipboard.ClipboardTarget
import io.github.yurisismotto.pliwee.clipboard.ClipboardText
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.store.TrustStore
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * CLIP-SEC-15 on Android: what does and does not survive on disk.
 *
 * Instrumented rather than JVM because it reads the **real** trust-store file
 * from the app's real `filesDir`, using the platform's own `org.json`. A JVM
 * test with a stubbed `Context` would be asserting against a file this app
 * never writes.
 *
 * Two claims are made here, and they pull in opposite directions on purpose:
 * clipboard *content* must not be findable anywhere in the app's private
 * storage, and clipboard *policy* must survive a restart. A store that
 * persisted neither would pass a naive "no content" test while quietly losing
 * the person's settings.
 */
@RunWith(AndroidJUnit4::class)
class ClipboardPersistenceTest {

    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    /** A string that appears nowhere else in the app or its resources. */
    private val canary = "CANARY-ANDROID-PERSIST-${System.nanoTime()}"

    private fun fingerprint(byte: Byte) = Fingerprint(ByteArray(32) { byte })

    /** Every byte of the app's private storage, as text. */
    private fun everythingOnDisk(): List<Pair<String, String>> =
        context.filesDir.walkTopDown()
            .plus(context.cacheDir.walkTopDown())
            .plus(
                context.applicationInfo.dataDir
                    ?.let { java.io.File(it, "shared_prefs").walkTopDown() }
                    ?: emptySequence(),
            )
            .filter { it.isFile }
            .map { it.absolutePath to runCatching { it.readText() }.getOrDefault("") }
            .toList()

    @Test
    fun clipboard_content_is_never_written_to_the_trust_store_or_anywhere_else() = runBlocking {
        val store = TrustStore(context)
        val peer = fingerprint(0x5c)

        store.addPeer(
            TrustStore.TrustedPeer(
                deviceId = "persistence-test-device",
                deviceName = "Test Computer",
                fingerprint = peer,
                pairedAtUnix = 1_700_000_000,
                grantedCapabilities = setOf(ClipboardCapability.ID),
                addressHints = emptyList(),
            ),
        )

        // A clip that is applied, and a clip that is held — the two states in
        // which clipboard text exists at all.
        val clipboard = RecordingTarget()
        val sync = ClipboardSync(
            systemClipboard = clipboard,
            authorizer = { store.clipboardPolicyFor(it) },
            localDeviceId = store.deviceId,
        )

        // auto-receive off: held in memory.
        sync.handleControl(
            peer,
            "desktop",
            "Test Computer",
            updatePayload(canary),
        )
        assertEquals(1, sync.pendingClips.value.size)

        // Then applied.
        store.setClipboardPolicy(peer, ClipboardPolicy(autoReceive = true))
        sync.applyPending(peer)

        // Force a store write, so the assertion is against a file that has
        // actually been rewritten since the clip existed.
        store.setClipboardPolicy(
            peer,
            ClipboardPolicy(allowSend = false, autoReceive = true),
        )

        val files = everythingOnDisk()
        assertTrue("no files were inspected; the audit proved nothing", files.isNotEmpty())
        for ((path, text) in files) {
            assertFalse("$path contains clipboard content", text.contains(canary))
        }

        // And the policy *is* there: settings persist, content does not.
        val reloaded = TrustStore(context).peer(peer)
        assertFalse("the policy must survive", reloaded!!.clipboardPolicy.allowSend)
        assertTrue(reloaded.clipboardPolicy.autoReceive)
        assertTrue(reloaded.allows(ClipboardCapability.ID))

        // Clean up so a later run starts from a known state, through the
        // lifecycle the product actually has: revoke, then take the row off
        // the list. What is left is a tombstone — a fingerprint and two flags,
        // with no name, no grant and no policy — which is harmless and
        // invisible, and which `addPeer` replaces wholesale on the next run.
        // There is deliberately no hard-delete API to call instead.
        cleanUp(peer)
    }

    /**
     * Withdrawing trust takes the clipboard policy with it, on disk.
     *
     * This test used to be `a_forgotten_computer_leaves_no_clipboard_policy_behind`
     * and it asserted that deleting the record left nothing behind. That is no
     * longer how the product works, and the old shape would have been the weaker
     * claim anyway: "the policy is gone because the whole record is gone" says
     * nothing about a record that *stays*.
     *
     * The rule now is stronger and is what this asserts: **revoking scrubs the
     * grants and both policies from the record itself**, and the record it
     * leaves behind — revoked, then tombstoned — keeps answering `DENIED`
     * without relying on the row having vanished. That matters precisely
     * because the record survives: a stale `autoReceive` sitting on a revoked
     * peer is consent waiting to be resurrected by a re-pair.
     */
    @Test
    fun revoking_a_computer_scrubs_its_clipboard_policy_and_the_tombstone_keeps_it_denied() =
        runBlocking {
            val store = TrustStore(context)
            val peer = fingerprint(0x6d)
            store.addPeer(
                TrustStore.TrustedPeer(
                    deviceId = "revoke-test-device",
                    deviceName = "Temporary",
                    fingerprint = peer,
                    pairedAtUnix = 1_700_000_000,
                    grantedCapabilities = setOf(ClipboardCapability.ID),
                    addressHints = emptyList(),
                    clipboardPolicy = ClipboardPolicy(autoReceive = true, autoSend = true),
                ),
            )
            assertTrue(store.clipboardPolicyFor(peer).mayAutoReceive())

            assertTrue("the record must be there to revoke", store.revokePeer(peer))

            // Denied, read back from a fresh store so the assertion is against
            // the file rather than against anything held in memory.
            val afterRevoke = TrustStore(context)
            assertEquals(ClipboardPolicy.DENIED, afterRevoke.clipboardPolicyFor(peer))

            // And denied because the policy was *scrubbed*, not merely gated
            // behind the revoked flag. `peer()` excludes revoked records, so
            // the stored record is read through `peerRecord`.
            val record = afterRevoke.peerRecord(peer)!!
            assertTrue("revoking keeps the record", record.revoked)
            assertFalse("the row stays visible until the person removes it", record.hidden)
            assertEquals(ClipboardPolicy.DENIED, record.clipboardPolicy)
            assertFalse(record.clipboardPolicy.autoReceive)
            assertFalse(record.clipboardPolicy.autoSend)
            assertTrue(record.grantedCapabilities.isEmpty())

            // Taking it off the list keeps every one of those answers.
            assertTrue(afterRevoke.hideRevokedPeer(peer))
            val afterRemove = TrustStore(context)
            assertEquals(ClipboardPolicy.DENIED, afterRemove.clipboardPolicyFor(peer))
            val tombstone = afterRemove.peerRecord(peer)!!
            assertTrue("a tombstone is still a record, not a deletion", tombstone.revoked)
            assertTrue(tombstone.hidden)
            assertEquals("", tombstone.deviceName)
            assertEquals(ClipboardPolicy.DENIED, tombstone.clipboardPolicy)
            assertTrue(tombstone.grantedCapabilities.isEmpty())
        }

    @Test
    fun a_peer_without_the_grant_is_denied_whatever_its_stored_policy_says() = runBlocking {
        val store = TrustStore(context)
        val peer = fingerprint(0x7e)
        store.addPeer(
            TrustStore.TrustedPeer(
                deviceId = "ungranted-device",
                deviceName = "Ungranted",
                fingerprint = peer,
                pairedAtUnix = 1_700_000_000,
                // Everything the policy can express, and no grant.
                grantedCapabilities = emptySet(),
                addressHints = emptyList(),
                clipboardPolicy = ClipboardPolicy(
                    allowSend = true,
                    allowReceive = true,
                    autoSend = true,
                    autoReceive = true,
                ),
            ),
        )

        assertEquals(
            "the grant gates the policy, not the other way round",
            ClipboardPolicy.DENIED,
            store.clipboardPolicyFor(peer),
        )

        cleanUp(peer)
    }

    /**
     * Returns a temporary test peer to a harmless state.
     *
     * The lifecycle, not a hard delete: `revokePeer` clears the grants and both
     * policies, `hideRevokedPeer` reduces what is left to a fingerprint and two
     * flags. The record stays — that is the product's design, and the point of
     * it — but it holds nothing, is invisible to every screen and to
     * `TrustStore.peers`, and is replaced outright by `addPeer` if the test
     * runs again.
     */
    private fun cleanUp(peer: Fingerprint) {
        val store = TrustStore(context)
        store.revokePeer(peer)
        store.hideRevokedPeer(peer)
    }

    /** A clipboard that never touches the real one. */
    private class RecordingTarget : ClipboardTarget {
        var current: ClipboardTarget.ReadClip? = null

        override fun read(): Result<ClipboardTarget.ReadClip> =
            current?.let { Result.success(it) }
                ?: Result.failure(IllegalStateException("empty"))

        override fun write(
            text: ClipboardText,
            sensitive: Boolean,
            label: String,
        ): ClipboardTarget.WriteResult {
            current = ClipboardTarget.ReadClip(text, sensitive)
            return ClipboardTarget.WriteResult.Applied
        }
    }

    private fun updatePayload(text: String) =
        io.github.yurisismotto.pliwee.proto.capabilities.ClipboardControl.newBuilder()
            .setUpdate(
                io.github.yurisismotto.pliwee.proto.capabilities.ClipboardUpdate.newBuilder()
                    .setEventId(
                        com.google.protobuf.ByteString.copyFrom(ByteArray(16) { 0x42 }),
                    )
                    .setOriginDeviceId("desktop")
                    .setTextUtf8(text)
                    .setContentHash(
                        com.google.protobuf.ByteString.copyFrom(
                            ClipboardText.contentHash(text),
                        ),
                    )
                    .setSensitiveHint(false)
                    .setTimestampUnixMs(1_700_000_000_000),
            )
            .build()
            .toByteString()
}
