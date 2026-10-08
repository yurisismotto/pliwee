package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.files.FailureReason
import io.github.yurisismotto.pliwee.files.FileTransferManager
import io.github.yurisismotto.pliwee.files.FilesOpen
import io.github.yurisismotto.pliwee.files.MimeTypes
import io.github.yurisismotto.pliwee.files.OpenAction
import io.github.yurisismotto.pliwee.files.TransferState
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.FilesMapping
import io.github.yurisismotto.pliwee.ui.FilesMapping.Direction
import io.github.yurisismotto.pliwee.ui.FilesMapping.FileStatus
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Files screen tells the truth about transfers.
 *
 * Every claim that screen makes — this was sent, that was declined, this one
 * can be opened — is produced by [FilesMapping] or [FilesOpen], both of which
 * are plain Kotlin over plain data. So the whole surface is asserted here, on
 * the JVM, in milliseconds: no device, no emulator, and no screenshot
 * comparison anywhere.
 *
 * Three properties run through all of it, and each is a defect this product
 * has already had once:
 *
 *  * **a filename is not an identity.** UX-DEBT-01 was `transfers.filter {
 *    it.filename == name }`, which made two files called `report.pdf` into
 *    one transfer. T3 and T19 hold that line;
 *  * **a device name is not an identity.** Two computers may legitimately
 *    call themselves the same thing, and only the fingerprint separates them.
 *    T4 and T20;
 *  * **a status word has to be earned.** No amount of progress promotes a
 *    failure, and nothing infers success from a file happening to exist.
 *    T5, T6 and T25.
 */
class FilesUxTest {

    // -----------------------------------------------------------------------
    // The world
    // -----------------------------------------------------------------------

    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private fun peer(name: String, seed: Int) = TrustStore.TrustedPeer(
        deviceId = "device-$seed",
        deviceName = name,
        fingerprint = fingerprint(seed),
        pairedAtUnix = 1_700_000_000L + seed,
        grantedCapabilities = setOf(FilesCapability.ID),
        addressHints = listOf(AddressHint("192.168.68.${70 + seed}", 55432)),
    )

    private val fedora = peer("Fedora", 1)
    private val debian = peer("omnibridge-d13", 2)
    private val peers = listOf(fedora, debian)

    private var ordinal = 0L

    private fun transfer(
        id: String,
        filename: String = "report.pdf",
        sending: Boolean = true,
        state: TransferState = TransferState.COMPLETED,
        failure: FailureReason? = null,
        peer: TrustStore.TrustedPeer = fedora,
        sizeBytes: Long = 2_400_000,
        bytesTransferred: Long = 2_400_000,
        savedName: String? = null,
        mimeType: String = "application/pdf",
        hasOpenTarget: Boolean = true,
        accessLost: Boolean = false,
        sequence: Long = ++ordinal,
        settledSequence: Long? = if (state.isTerminal) ++ordinal else null,
    ) = FileTransferManager.TransferUi(
        transferId = id,
        peer = peer.fingerprint,
        filename = filename,
        sizeBytes = sizeBytes,
        bytesTransferred = bytesTransferred,
        sending = sending,
        state = state,
        failure = failure,
        savedName = savedName,
        mimeType = mimeType,
        hasOpenTarget = hasOpenTarget,
        accessLost = accessLost,
        sequence = sequence,
        settledSequence = settledSequence,
    )

    private fun rows(vararg transfers: FileTransferManager.TransferUi) =
        FilesMapping.build(transfers.toList(), peers)

    private fun only(transfer: FileTransferManager.TransferUi) =
        FilesMapping.row(transfer, FilesMapping.peerLabels(peers))

    // =======================================================================
    // T1, T2 — direction
    // =======================================================================

    @Test
    fun `T1 an outgoing transfer in flight maps to the Sent direction`() {
        val row = only(transfer("a1", sending = true, state = TransferState.TRANSFERRING))
        assertEquals(Direction.SENT, row.direction)
        // In flight, so the status is the verb rather than the past tense. A
        // transfer still moving must never read as delivered.
        assertEquals(FileStatus.SENDING, row.status)
        assertTrue(row.isActive)
    }

    @Test
    fun `T2 an incoming transfer in flight maps to the Received direction`() {
        val row = only(transfer("b1", sending = false, state = TransferState.TRANSFERRING))
        assertEquals(Direction.RECEIVED, row.direction)
        assertEquals(FileStatus.RECEIVING, row.status)
        assertTrue(row.isActive)
    }

    // =======================================================================
    // T3, T19 — a filename is not an identity
    // =======================================================================

    @Test
    fun `T3 two transfers of the same filename are two rows`() {
        val first = transfer("aa11", filename = "report.pdf")
        val second = transfer("bb22", filename = "report.pdf")
        val ui = rows(first, second)

        assertEquals(2, ui.recent.size)
        assertEquals(
            "rows are keyed by transfer id, so two same-named files stay apart",
            setOf("aa11", "bb22"),
            ui.recent.map { it.transferId }.toSet(),
        )
        assertEquals(2, ui.recent.map { it.key }.toSet().size)
    }

    @Test
    fun `T19 nothing on this screen looks a transfer up by its name`() {
        // The defect this forbids is a real one: `filter { it.filename == name }`
        // is what UX-DEBT-01 removed from the send surface, and a Files screen
        // is the obvious place for it to come back.
        val source =
            File("src/main/java/io/github/yurisismotto/pliwee/ui/FilesMapping.kt").readText()
        for (smell in listOf(
            "filename ==",
            "displayName ==",
            "filename.equals",
            "first { it.filename",
        )) {
            assertFalse("FilesMapping matches transfers by name: $smell", source.contains(smell))
        }

        // And the positive half: two rows differing only by id stay distinct
        // while agreeing about everything a person can see.
        val a = transfer("aa11", filename = "same.pdf", sequence = 1, settledSequence = 2)
        val b = transfer("bb22", filename = "same.pdf", sequence = 3, settledSequence = 4)
        assertNotEquals(only(a).transferId, only(b).transferId)
        assertEquals(only(a).displayName, only(b).displayName)
    }

    // =======================================================================
    // T4, T20 — a device name is not an identity
    // =======================================================================

    @Test
    fun `T4 two computers with the same name stay separate rows`() {
        val one = peer("Fedora", 7)
        val two = peer("Fedora", 8)

        val ui = FilesMapping.build(
            listOf(transfer("aa11", peer = one), transfer("bb22", peer = two)),
            listOf(one, two),
        )

        assertEquals(2, ui.recent.size)
        assertEquals(
            "the rows carry different fingerprints even though the names match",
            2,
            ui.recent.map { it.peerHex }.toSet().size,
        )
        // They are told apart on screen by the established short fingerprint,
        // and never by an IP address — that names a position on a network
        // rather than a machine, and it changes on its own.
        val labels = ui.recent.map { it.peerLabel }.toSet()
        assertEquals(2, labels.size)
        assertTrue(labels.toString(), labels.all { it.startsWith("Fedora") })
        assertTrue(labels.toString(), labels.none { it.contains("192.168") })
    }

    @Test
    fun `T20 a lone peer keeps its plain name and nothing routes on it`() {
        val row = only(transfer("aa11", peer = fedora))
        assertEquals("Fedora", row.peerLabel)
        assertEquals(fedora.fingerprint.toHex(), row.peerHex)

        // A peer absent from the list — revoked mid-transfer — still gets a
        // row, identified by its fingerprint. Dropping it would be hiding a
        // transfer that really happened.
        val stranger = peer("Gone", 9)
        val orphan = FilesMapping.build(listOf(transfer("cc33", peer = stranger)), peers)
        assertEquals(1, orphan.recent.size)
        assertEquals(stranger.fingerprint.toHex(), orphan.recent.single().peerHex)
        assertEquals(stranger.fingerprint.toDisplayShort(), orphan.recent.single().peerLabel)
    }

    // =======================================================================
    // T5, T6 — success is only ever the state machine's word
    // =======================================================================

    @Test
    fun `T5 an outgoing transfer reads as Sent only when it really completed`() {
        assertEquals(
            FileStatus.SENT,
            only(transfer("a", sending = true, state = TransferState.COMPLETED)).status,
        )
        // Every byte moved, and then it failed. That is not "Sent".
        assertEquals(
            FileStatus.FAILED,
            only(
                transfer(
                    "b",
                    sending = true,
                    state = TransferState.FAILED,
                    failure = FailureReason.INTEGRITY,
                    bytesTransferred = 2_400_000,
                ),
            ).status,
        )
    }

    @Test
    fun `T6 an incoming transfer reads as Received only when it really completed`() {
        assertEquals(
            FileStatus.RECEIVED,
            only(transfer("a", sending = false, state = TransferState.COMPLETED)).status,
        )
        // VERIFYING means every byte is in and the hash is being checked.
        // That is not "Received", and the difference is the whole integrity
        // story: unverified bytes are never presented as a finished file.
        assertEquals(
            FileStatus.RECEIVING,
            only(transfer("b", sending = false, state = TransferState.VERIFYING)).status,
        )
    }

    // =======================================================================
    // T7 to T11 — every ending keeps its own name
    // =======================================================================

    @Test
    fun `T7 a decline stays Declined`() {
        assertEquals(
            FileStatus.DECLINED,
            only(
                transfer(
                    "a",
                    state = TransferState.CANCELLED,
                    failure = FailureReason.DECLINED_BY_USER,
                ),
            ).status,
        )
    }

    @Test
    fun `T8 a cancellation stays Cancelled`() {
        assertEquals(
            FileStatus.CANCELLED,
            only(
                transfer(
                    "a",
                    state = TransferState.CANCELLED,
                    failure = FailureReason.CANCELLED_BY_USER,
                ),
            ).status,
        )
    }

    @Test
    fun `T9 a timeout stays Timed out`() {
        assertEquals(
            FileStatus.TIMED_OUT,
            only(transfer("a", state = TransferState.FAILED, failure = FailureReason.TIMED_OUT))
                .status,
        )
    }

    @Test
    fun `T10 a lost session stays Disconnected`() {
        assertEquals(
            FileStatus.DISCONNECTED,
            only(transfer("a", state = TransferState.FAILED, failure = FailureReason.TRANSPORT))
                .status,
        )
    }

    @Test
    fun `T11 anything else stays Failed`() {
        for (reason in listOf(
            FailureReason.INTEGRITY,
            FailureReason.STORAGE,
            FailureReason.BAD_METADATA,
            FailureReason.NOT_AUTHORIZED,
            FailureReason.TOO_LARGE,
            FailureReason.REVOKED,
            null,
        )) {
            assertEquals(
                "a $reason failure",
                FileStatus.FAILED,
                only(transfer("a", state = TransferState.FAILED, failure = reason)).status,
            )
        }
    }

    @Test
    fun `the two vocabularies agree about what is finished`() {
        // A state the machine calls terminal must not turn up in the Active
        // section, and a live one must not turn up in Recent.
        for (state in TransferState.entries) {
            for (sending in listOf(true, false)) {
                val row =
                    only(transfer("x", sending = sending, state = state, settledSequence = null))
                assertEquals("$state / sending=$sending", state.isTerminal, row.status.isTerminal)
                assertEquals("$state / sending=$sending", state.isActive, row.isActive)
            }
        }
    }

    // =======================================================================
    // T12, T13, T14 — the list itself
    // =======================================================================

    @Test
    fun `T12 progress updates move a row rather than adding one`() {
        val id = "aa11"
        var ui = rows(
            transfer(
                id,
                state = TransferState.TRANSFERRING,
                bytesTransferred = 0,
                sequence = 1,
                settledSequence = null,
            ),
        )
        assertEquals(1, ui.active.size)
        assertEquals(0, ui.active.single().progressPercent)

        // The manager republishes the whole list on every progress tick, so
        // the same transfer id must still be exactly one row afterwards.
        ui = rows(
            transfer(
                id,
                state = TransferState.TRANSFERRING,
                bytesTransferred = 1_200_000,
                sequence = 1,
                settledSequence = null,
            ),
        )
        assertEquals(1, ui.active.size)
        assertEquals(50, ui.active.single().progressPercent)
        assertEquals(id, ui.active.single().transferId)
    }

    @Test
    fun `T13 ordering is most recent first and deterministic`() {
        // Started first, settled last: a large file behind a small one.
        val big = transfer(
            "big",
            filename = "a-first-alphabetically.iso",
            sequence = 1,
            settledSequence = 40,
        )
        val small = transfer(
            "small",
            filename = "z-last-alphabetically.txt",
            sequence = 2,
            settledSequence = 20,
        )

        // Recent is ordered by when each one ended.
        assertEquals(listOf("big", "small"), rows(big, small).recent.map { it.transferId })
        // And the answer does not depend on the order they arrived in, which
        // a `HashMap`'s iteration order would have made it depend on.
        assertEquals(listOf("big", "small"), rows(small, big).recent.map { it.transferId })

        // Active is ordered by when each one started, which is a different
        // question with a different answer.
        val old =
            transfer("old", state = TransferState.TRANSFERRING, sequence = 1, settledSequence = null)
        val new =
            transfer("new", state = TransferState.TRANSFERRING, sequence = 9, settledSequence = null)
        assertEquals(listOf("new", "old"), rows(old, new).active.map { it.transferId })
    }

    @Test
    fun `T14 clearing finished transfers leaves the live ones alone`() {
        val live =
            transfer("live", state = TransferState.TRANSFERRING, sequence = 1, settledSequence = null)
        val done = transfer("done", state = TransferState.COMPLETED, sequence = 2, settledSequence = 3)

        val before = rows(live, done)
        assertEquals(1, before.active.size)
        assertEquals(1, before.recent.size)

        // `clearFinished` drops exactly the terminal ones from the manager;
        // what the screen shows afterwards is this.
        val after = rows(live)
        assertEquals(listOf("live"), after.active.map { it.transferId })
        assertTrue(after.recent.isEmpty())
        assertFalse(after.isEmpty)
    }

    // =======================================================================
    // T15 to T18 — whether Open is offered
    // =======================================================================

    @Test
    fun `T15 a received file that is still there can be opened`() {
        val row =
            only(transfer("a", sending = false, state = TransferState.COMPLETED, hasOpenTarget = true))
        assertEquals(OpenAction.Available, row.openAction)
        assertTrue(row.openAction.isOfferable)
    }

    @Test
    fun `T16 a received file that is gone says so and offers no button`() {
        // Two ways it can be gone: never retained, or proved missing since.
        val never =
            only(transfer("a", sending = false, state = TransferState.COMPLETED, hasOpenTarget = false))
        val deleted = only(
            transfer(
                "b",
                sending = false,
                state = TransferState.COMPLETED,
                hasOpenTarget = true,
                accessLost = true,
            ),
        )
        assertEquals(OpenAction.FileMissing, never.openAction)
        assertEquals(OpenAction.FileMissing, deleted.openAction)
        assertFalse(never.openAction.isOfferable)
        assertFalse(deleted.openAction.isOfferable)
    }

    @Test
    fun `T17 a sent file whose grant is still held can be opened`() {
        val row =
            only(transfer("a", sending = true, state = TransferState.COMPLETED, hasOpenTarget = true))
        assertEquals(OpenAction.Available, row.openAction)
    }

    @Test
    fun `T18 a sent file whose grant has lapsed reports the source, not the file`() {
        // The distinction matters to whoever reads it. The file is almost
        // certainly still on the device and Pliwee simply may not look at it
        // any more; saying it is missing would send someone hunting for
        // something that is not lost.
        val lapsed = only(
            transfer(
                "a",
                sending = true,
                state = TransferState.COMPLETED,
                hasOpenTarget = true,
                accessLost = true,
            ),
        )
        val never =
            only(transfer("b", sending = true, state = TransferState.COMPLETED, hasOpenTarget = false))
        assertEquals(OpenAction.SourceUnavailable, lapsed.openAction)
        assertEquals(OpenAction.SourceUnavailable, never.openAction)
        assertNotEquals(OpenAction.FileMissing, lapsed.openAction)
    }

    @Test
    fun `nothing unfinished is openable, however far it got`() {
        for (state in TransferState.entries.filter { it != TransferState.COMPLETED }) {
            for (sending in listOf(true, false)) {
                assertEquals(
                    "$state / sending=$sending must not offer Open",
                    OpenAction.NotApplicable,
                    FilesOpen.decide(sending, state, hasTarget = true),
                )
            }
        }
    }

    @Test
    fun `an unchecked target is not assumed to be broken`() {
        // `accessLost` defaults to false because it is a negative that has to
        // be established. A row must not claim a file is missing merely
        // because nobody has looked yet.
        assertEquals(
            OpenAction.Available,
            FilesOpen.decide(sending = false, state = TransferState.COMPLETED, hasTarget = true),
        )
    }

    // =======================================================================
    // T21, T22 — what the decision and the row may contain
    // =======================================================================

    @Test
    fun `T21 the open decision carries no file content and no URI`() {
        // Structural, and checked at the type level: every answer is a data
        // object, so none of them has anywhere to put a payload. This is the
        // assertion that stops somebody adding `Available(uri, mime)` later.
        for (action in listOf(
            OpenAction.Available,
            OpenAction.SourceUnavailable,
            OpenAction.FileMissing,
            OpenAction.NoViewer,
            OpenAction.NotApplicable,
        )) {
            // Java reflection rather than Kotlin's: `kotlin-reflect` is not
            // on this module's test classpath, and adding a megabyte of it to
            // ask one question would be a poor trade. A `data object`
            // compiles to a class whose only field is the static INSTANCE, so
            // "has no instance field" is the same assertion.
            val payload = action.javaClass.declaredFields
                .filterNot { java.lang.reflect.Modifier.isStatic(it.modifiers) }
            assertTrue(
                "${action.javaClass.simpleName} carries $payload; it must carry nothing",
                payload.isEmpty(),
            )
        }

        val source = File("src/main/java/io/github/yurisismotto/pliwee/files/OpenAction.kt")
            .readText()
            .replace(Regex("(?s)/\\*\\*.*?\\*/"), "")
            .lines().filterNot { it.trim().startsWith("//") }.joinToString("\n")
        assertFalse("OpenAction names a Uri", source.contains("Uri"))
        assertFalse("OpenAction names a path", source.contains("path", ignoreCase = true))
        assertFalse("OpenAction names bytes", source.contains("ByteArray"))
    }

    @Test
    fun `T22 a display row contains no content URI and no absolute path`() {
        val row = only(
            transfer(
                "aa11",
                filename = "report.pdf",
                savedName = "report (1).pdf",
                mimeType = "application/pdf",
            ),
        )
        val rendered = row.toString()
        for (leak in listOf("content://", "file://", "/storage/", "/data/")) {
            assertFalse("$leak in $rendered", rendered.contains(leak))
        }

        // The row shows what the file was actually saved as rather than what
        // was asked for: MediaStore renames a duplicate instead of
        // overwriting, and a row showing the offered name would send someone
        // looking for a file that is not there.
        assertEquals("report (1).pdf", row.displayName)

        // And the type itself has no field that could hold a URI.
        val fields = FilesMapping.FileRow::class.java.declaredFields.map { it.name }
        assertFalse(fields.toString(), fields.any { it.contains("uri", ignoreCase = true) })
        assertFalse(fields.toString(), fields.any { it.contains("path", ignoreCase = true) })
    }

    // =======================================================================
    // T23, T24 — the type handed to Android
    // =======================================================================

    @Test
    fun `T23 an unknown or unusable MIME type falls back safely`() {
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize(null))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize(""))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("   "))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("not-a-media-type"))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("a/b/c"))

        // The peer's string is what selects an application to open the file,
        // so it is the one offer field that chooses code to run.
        assertEquals(
            "a wildcard would widen the chooser to everything",
            MimeTypes.FALLBACK,
            MimeTypes.sanitize("*/*"),
        )
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("image/*"))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("text/plain\nX-Injected: 1"))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.sanitize("../../etc/passwd"))

        // What survives, survives unchanged but normalised.
        assertEquals("application/pdf", MimeTypes.sanitize("application/pdf"))
        assertEquals("application/pdf", MimeTypes.sanitize("APPLICATION/PDF"))
        assertEquals("text/plain", MimeTypes.sanitize("text/plain; charset=utf-8"))

        // Most trustworthy source first, and a bad one never shadows a good one.
        assertEquals("image/png", MimeTypes.firstUsable(null, "image/png"))
        assertEquals("image/png", MimeTypes.firstUsable("*/*", "image/png"))
        assertEquals("image/jpeg", MimeTypes.firstUsable("image/jpeg", "application/x-evil "))
        assertEquals(MimeTypes.FALLBACK, MimeTypes.firstUsable(null, null))
    }

    @Test
    fun `T24 no viewer is a truthful outcome with words of its own`() {
        // It is reachable only from an actual attempt — nothing predicts it —
        // so what is asserted here is that it exists as a distinct answer and
        // is never confused with the file being absent.
        assertNotEquals(OpenAction.NoViewer, OpenAction.FileMissing)
        assertNotEquals(OpenAction.NoViewer, OpenAction.SourceUnavailable)
        assertFalse(OpenAction.NoViewer.isOfferable)

        val strings = File("src/main/res/values/strings.xml").readText()
        for (name in listOf(
            "files_open_no_viewer",
            "files_open_file_missing",
            "files_open_source_unavailable",
        )) {
            assertTrue("$name is not a string resource", strings.contains("name=\"$name\""))
        }
        // Three distinct sentences, not one shared one.
        val sentences = Regex("""<string name="files_open_[a-z_]+">([^<]*)</string>""")
            .findAll(strings).map { it.groupValues[1] }.toList()
        assertEquals(3, sentences.size)
        assertEquals("each failure says something different", 3, sentences.toSet().size)
    }

    // =======================================================================
    // T25 — nothing promotes a failure
    // =======================================================================

    @Test
    fun `T25 no session or progress state can turn a failure into a success`() {
        val failed = transfer(
            "aa11",
            sending = true,
            state = TransferState.FAILED,
            failure = FailureReason.TRANSPORT,
            bytesTransferred = 2_400_000,
            sizeBytes = 2_400_000,
            hasOpenTarget = true,
        )
        val row = only(failed)

        assertEquals(FileStatus.DISCONNECTED, row.status)
        assertNotEquals(FileStatus.SENT, row.status)
        // Every byte moved, and it still cannot be opened.
        assertEquals(OpenAction.NotApplicable, row.openAction)
        assertNull("a terminal row shows no progress bar", row.progressPercent)

        // The state machine itself refuses to leave a terminal state, which
        // is the property this row relies on.
        assertFalse(TransferState.FAILED.canTransitionTo(TransferState.COMPLETED))
        assertFalse(TransferState.CANCELLED.canTransitionTo(TransferState.COMPLETED))
    }

    // =======================================================================
    // T26 to T28 — retry is deliberately not on this screen
    // =======================================================================

    @Test
    fun `the Files screen offers no Send again, because it could not do so safely`() {
        // Retry lives on the share-sheet surface, where the source URI's read
        // grant is still live because the Activity that received the share is
        // still on screen. On Files that grant is normally gone — which is
        // exactly what T18 asserts — so a "Send again" here would either fail
        // or have to reach for a persistable permission this product does not
        // take. It is left out rather than faked; see the sprint report.
        val screen =
            File("src/main/java/io/github/yurisismotto/pliwee/ui/FilesScreen.kt").readText()
        assertFalse(screen.contains("Send again"))
        assertFalse(screen.contains("onRetry"))
        assertFalse(
            "nothing on this screen may start a transfer",
            screen.contains(".offer("),
        )

        // And a terminal row goes on saying what happened rather than turning
        // into a button that would replace it.
        val declined = only(
            transfer(
                "aa11",
                state = TransferState.CANCELLED,
                failure = FailureReason.DECLINED_BY_USER,
            ),
        )
        assertEquals(FileStatus.DECLINED, declined.status)
        assertEquals(OpenAction.NotApplicable, declined.openAction)
    }

    // =======================================================================
    // A pending offer never outlives its own transfer
    // =======================================================================

    @Test
    fun `an offer card is withdrawn when its transfer ends`() {
        // Certified on hardware: killing the desktop daemon while an offer
        // sat on screen left "Accept / Reject" above a row that already read
        // Disconnected. Answering it failed closed — `transition` refuses to
        // leave a terminal state — but the screen was making two statements
        // about one file and one of them was false.
        val manager =
            File("src/main/java/io/github/yurisismotto/pliwee/files/FileTransferManager.kt")
                .readText()
        val finish = manager.substringAfter("private fun finish(").substringBefore("\n    }")
        assertTrue(
            "finish() must withdraw any offer still pending for the transfer it is ending",
            finish.contains("_pending.value = _pending.value.filterNot"),
        )
        assertTrue(
            "and release the coroutine waiting on it",
            finish.contains("answer.complete(false)"),
        )

        // The state machine is what makes answering a dead offer safe, and it
        // is asserted rather than assumed.
        for (terminal in TransferState.entries.filter { it.isTerminal }) {
            assertFalse(
                "$terminal must not be able to start transferring",
                terminal.canTransitionTo(TransferState.TRANSFERRING),
            )
        }
    }

    // =======================================================================
    // Logging
    // =======================================================================

    @Test
    fun `nothing on the files path logs a URI, a path, or a peer's filename`() {
        val sources = listOf(
            "src/main/java/io/github/yurisismotto/pliwee/ui/FilesMapping.kt",
            "src/main/java/io/github/yurisismotto/pliwee/ui/FilesScreen.kt",
            "src/main/java/io/github/yurisismotto/pliwee/files/OpenAction.kt",
            "src/main/java/io/github/yurisismotto/pliwee/files/MimeTypes.kt",
        ).map { File(it) }

        for (source in sources) {
            val code = source.readText()
                .replace(Regex("(?s)/\\*\\*.*?\\*/"), "")
                .lines().filterNot { it.trim().startsWith("//") }.joinToString("\n")
            assertFalse("${source.name} logs at all", Regex("""\bLog\.[vdiwe]\(""").containsMatchIn(code))
        }

        // The one place on this path that does log says only which transfer
        // it was and which way it ran.
        val manager =
            File("src/main/java/io/github/yurisismotto/pliwee/files/FileTransferManager.kt")
                .readText()
        val openLog = manager.substringAfter("private fun markAccessLost")
            .substringBefore("\n    }")
            // The comment inside that function says the words "no URI, no
            // filename" in order to explain itself. Prose is not a log line,
            // and a check that could not tell them apart would push the next
            // person into deleting the explanation.
            .lines().filterNot { it.trim().startsWith("//") }.joinToString("\n")
        assertTrue("the log names the transfer by its id prefix", openLog.contains("id.take(8)"))
        assertFalse("the open path must not log a URI", openLog.contains("uri", ignoreCase = true))
        assertFalse("the open path must not log a filename", openLog.contains("filename"))
        assertFalse("the open path must not log a peer fingerprint in full", openLog.contains("toHex"))
    }
}
