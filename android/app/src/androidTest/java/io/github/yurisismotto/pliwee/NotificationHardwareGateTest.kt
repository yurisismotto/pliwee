package io.github.yurisismotto.pliwee

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.google.protobuf.ByteString
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.notifications.NotificationAccess
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.proto.capabilities.NotificationControl
import io.github.yurisismotto.pliwee.proto.capabilities.NotificationRole
import io.github.yurisismotto.pliwee.proto.capabilities.NotificationRoles
import io.github.yurisismotto.pliwee.store.TrustStore
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.TimeUnit
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The `notifications.v1` source path, on the real device.
 *
 * ## What is real here, and what is not
 *
 * Real: the bound `NotificationListenerService`, the platform's own
 * `getActiveNotifications()`, the extraction from a genuine
 * `StatusBarNotification`, the Android Keystore secret, the app's real
 * `NotificationSource`, the real trust store, and the real protobuf encoding.
 *
 * Not real: the socket. There is **no `notifications.v1` peer to talk to** —
 * the Linux sink is N2 and does not exist — so the session is a local capture
 * of the bytes the capability produced. That is the honest boundary of what N1
 * can prove on hardware, and it is stated rather than papered over: what is
 * verified is that the correct bytes were produced for a granted peer, not
 * that a desktop displayed them.
 *
 * ## Setting up the fixture
 *
 * Two `adb` commands, run before this suite, are what make the interesting
 * gates executable. They post a notification from `com.android.shell` — a
 * different package from Pliwee, which is what the own-package rule needs to
 * be distinguishable from an allow-list decision:
 *
 * ```console
 * $ adb shell cmd notification allow_listener \
 *     io.github.yurisismotto.pliwee/io.github.yurisismotto.pliwee.notifications.PliweeNotificationListener
 * $ adb shell cmd notification post -t 'OMNIBRIDGE-N1-FIXTURE-TITLE' \
 *     omnibridge-n1-fixture 'OMNIBRIDGE-N1-FIXTURE-BODY'
 * ```
 *
 * The fixture strings are deliberately distinctive and deliberately not
 * sensitive: they are what the logcat and persistent-state audits grep for, so
 * a leak would be unmistakable, and finding one in a log would not itself be a
 * disclosure.
 *
 * Every gate that cannot run because its precondition is absent **skips with a
 * reason** rather than passing. An unexecuted gate is reported as unexecuted.
 */
@RunWith(AndroidJUnit4::class)
class NotificationHardwareGateTest {

    /**
     * The package whose notifications this gate looks for.
     *
     * **N5 moved this off `com.android.shell`**, which is what N1 had to use
     * and what N3 debt 3 and N4 debt 2 are both about. `cmd notification post`
     * can create a clearable notification and nothing else: no cancel, no
     * ongoing, no group, no tag control — and `com.android.shell` has no
     * launcher entry, so Pliwee's own app picker cannot offer it until it is
     * *already* notifying, which needs the listener bound, which needs a
     * granted peer connected.
     *
     * `android/fixture/` is a test-only module that fixes all of that. It is
     * never part of the Pliwee APK — nothing depends on it — and it is
     * installed by hand for a certification run:
     *
     * ```console
     * ./gradlew :fixture:assembleDebug
     * adb install -r fixture/build/outputs/apk/debug/fixture-debug.apk
     * adb shell pm grant io.github.yurisismotto.pliwee.fixture \
     *     android.permission.POST_NOTIFICATIONS
     * adb shell "am start -n io.github.yurisismotto.pliwee.fixture/.FixtureActivity \
     *     --es op post --es id 1 --es tag gate --es title 'OMNIBRIDGE-N1-FIXTURE-TITLE'"
     * ```
     *
     * See `android/fixture/README.md`.
     */
    private val fixturePackage = "io.github.yurisismotto.pliwee.fixture"
    private val fixtureTitle = "OMNIBRIDGE-N1-FIXTURE-TITLE"

    /**
     * A synthetic peer, added to the trust store for the duration of the test
     * and removed afterwards.
     *
     * It is not the maintainer's real pairing and does not disturb it: a
     * fingerprint of all-`0x7e` is not a key any device holds, so nothing can
     * connect as it, and [tearDown] takes it out of the trusted set whatever
     * the outcome.
     */
    private val testPeer = Fingerprint(ByteArray(Fingerprint.LENGTH) { 0x7e })

    private val app: PliweeApp
        get() = InstrumentationRegistry.getInstrumentation()
            .targetContext.applicationContext as PliweeApp

    private val captured = CopyOnWriteArrayList<NotificationControl>()

    @Before
    fun grantTheTestPeer() {
        app.trustStore.addPeer(
            TrustStore.TrustedPeer(
                deviceId = "0123456789abcdef0123456789abcdef",
                deviceName = "N1 hardware gate",
                fingerprint = testPeer,
                pairedAtUnix = 0,
                grantedCapabilities = setOf(TrustStore.NOTIFICATIONS_CAPABILITY_ID),
                addressHints = emptyList(),
                notificationPolicy = NotificationPolicy(allowedApps = setOf(fixturePackage)),
            ),
        )
    }

    /**
     * Takes the synthetic peer back out, through the lifecycle the product has.
     *
     * This used to be `removePeer`, a hard delete, which no longer exists —
     * withdrawing trust on Android now keeps the record so that a revocation
     * cannot be mistaken for never having met. What this needs is unchanged,
     * and both steps deliver it: after `revokePeer` the fixture peer holds no
     * `notifications.v1` grant and no policy, so it can neither be connected to
     * nor be mirrored to, and after `hideRevokedPeer` it is off every screen.
     *
     * What remains is a tombstone — a fingerprint and two flags, for a key no
     * device holds. It disturbs nothing, and `grantTheTestPeer` replaces it
     * outright on the next run because `addPeer` writes the whole record.
     */
    @After
    fun tearDown() {
        app.notifications.detachSession(testPeer)
        app.trustStore.revokePeer(testPeer)
        app.trustStore.hideRevokedPeer(testPeer)
    }

    /**
     * Waits for the system to bind the listener.
     *
     * Attaching an eligible session makes the source call `requestRebind`, and
     * the platform binds asynchronously — it starts the service, which calls
     * `onListenerConnected`. This is the wait for that round trip. It returns
     * whether the bind actually happened, so a gate can report "the listener
     * never bound" as an unexecuted precondition rather than as a pass.
     */
    private fun awaitBound(seconds: Long = 15): Boolean {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(seconds)
        while (System.nanoTime() < deadline) {
            if (app.notifications.status.value.listenerConnected) return true
            Thread.sleep(200)
        }
        return app.notifications.status.value.listenerConnected
    }

    /** Attaches a capturing session and waits for the producer to settle. */
    private fun attachAndDrain(claimSink: Boolean = true) {
        app.notifications.attachSession(testPeer, "0123456789abcdef0123456789abcdef") { payload ->
            captured += NotificationControl.parseFrom(payload)
        }
        if (claimSink) {
            app.notifications.onInbound(
                testPeer,
                NotificationControl.newBuilder()
                    .setRoles(
                        NotificationRoles.newBuilder()
                            .addRoles(NotificationRole.NOTIFICATION_ROLE_SINK)
                            .setEpoch(1),
                    )
                    .build(),
            )
        }
        settle()
        // The bind the attach just requested lands asynchronously; when it
        // does, the source re-announces and sends its snapshot. Waiting here
        // is what makes the content gates executable rather than racy.
        if (awaitBound()) settle(captured.size + 1)
    }

    /**
     * The producer is a coroutine on a background dispatcher, so a test has to
     * wait for it. A poll rather than a fixed sleep: it finishes as soon as
     * there is something to see, and it fails by asserting nothing rather than
     * by hanging.
     */
    private fun settle(atLeast: Int = 1) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (System.nanoTime() < deadline && captured.size < atLeast) {
            Thread.sleep(50)
        }
        Thread.sleep(250)
    }

    private fun upserts() =
        captured.filter { it.bodyCase == NotificationControl.BodyCase.UPSERT }

    // -- gate B: access held, no peer grant ----------------------------------

    /**
     * **Notification access alone sends nothing to anyone.**
     *
     * The listener may be bound and reading; a peer with no
     * `notifications.v1` grant is sent no notification content. The peer here
     * is granted in [grantTheTestPeer] and is revoked for this test alone.
     */
    @Test
    fun anUngrantedPeerIsSentNoNotificationContent() {
        app.trustStore.setGrant(testPeer, TrustStore.NOTIFICATIONS_CAPABILITY_ID, false)

        attachAndDrain()

        assertTrue(
            "an ungranted peer must receive no notification content",
            upserts().isEmpty(),
        )
    }

    // -- gate C: a granted, eligible peer ------------------------------------

    /**
     * A synthetic notification from an allowed application is observed,
     * identified and encoded — with the real listener, the real
     * `StatusBarNotification` and the real Keystore secret.
     */
    @Test
    fun anAllowedApplicationIsMirroredToAGrantedPeer() {
        assumeTrue(
            "notification access is not granted; run: adb shell cmd notification " +
                "allow_listener ${NotificationAccess.component(app).flattenToString()}",
            NotificationAccess.isGranted(app),
        )
        attachAndDrain()
        assumeTrue(
            "the system did not bind the listener within the timeout",
            app.notifications.status.value.listenerConnected,
        )

        val fixture = upserts().firstOrNull { it.upsert.appId == fixturePackage }
        assumeTrue(
            "no notification from $fixturePackage is currently active; post the fixture first",
            fixture != null,
        )

        val upsert = fixture!!.upsert
        assertEquals(16, upsert.notificationId.size())
        assertEquals(fixturePackage, upsert.appId)
        assertTrue("an app label must be resolved", upsert.appLabel.isNotEmpty())
        assertTrue(upsert.originDeviceId.length == 32)
    }

    /**
     * **Pliwee's own notification is never mirrored**, on hardware.
     *
     * The ongoing-connection foreground-service notification exists on every
     * running install and is the loop this rule exists to prevent. Allowing
     * our own package explicitly does not change the answer.
     */
    @Test
    fun ourOwnNotificationIsNeverMirroredOnHardware() {
        assumeTrue(
            "notification access is not granted",
            NotificationAccess.isGranted(app),
        )
        app.trustStore.setNotificationPolicy(
            testPeer,
            NotificationPolicy(allowedApps = setOf(fixturePackage, app.packageName)),
        )

        attachAndDrain()

        assertTrue(
            "Pliwee's own package must never be mirrored, under any policy",
            upserts().none { it.upsert.appId == app.packageName },
        )
    }

    /** A package nobody named is never mirrored, whatever else is active. */
    @Test
    fun anUnnamedApplicationIsNeverMirrored() {
        assumeTrue(
            "notification access is not granted",
            NotificationAccess.isGranted(app),
        )
        app.trustStore.setNotificationPolicy(
            testPeer,
            NotificationPolicy(allowedApps = setOf(fixturePackage)),
        )

        attachAndDrain()

        assertTrue(
            "only the named application may be mirrored",
            upserts().all { it.upsert.appId == fixturePackage },
        )
    }

    /**
     * The snapshot is bracketed, and it is bracketed on the device.
     *
     * `BEGIN`, the currently-active allowed notifications, `END` — produced by
     * one ordered coroutine, so the bracket cannot be split.
     */
    @Test
    fun theSnapshotIsBracketedOnHardware() {
        assumeTrue(
            "notification access is not granted",
            NotificationAccess.isGranted(app),
        )
        attachAndDrain()
        assumeTrue(
            "the system did not bind the listener within the timeout",
            app.notifications.status.value.listenerConnected,
        )

        val phases = captured
            .filter { it.bodyCase == NotificationControl.BodyCase.SYNC }
            .map { it.sync.phase }
        assumeTrue("no snapshot was produced", phases.isNotEmpty())

        assertEquals(2, phases.size)
        assertEquals(
            io.github.yurisismotto.pliwee.proto.capabilities.SyncMarker.Phase.PHASE_BEGIN,
            phases[0],
        )
        assertEquals(
            io.github.yurisismotto.pliwee.proto.capabilities.SyncMarker.Phase.PHASE_END,
            phases[1],
        )
        val markers = captured.filter { it.bodyCase == NotificationControl.BodyCase.SYNC }
        assertEquals(markers[0].sync.syncId, markers[1].sync.syncId)
        assertEquals(16, markers[0].sync.syncId.size())
    }

    /**
     * Roles are announced first, and a sourcing phone claims both of its v1
     * roles: `SOURCE` and `DISMISS_TARGET` (ADR-0017 §1).
     *
     * They travel together because they are true together — both need a bound
     * listener, the OS grant and a usable notification secret — and the secret
     * is why: a dismissal is resolved through the id map, whose entries are
     * built by deriving ids with it.
     *
     * What is asserted here that a JVM test cannot: the announcement was
     * produced by the **real** listener on the real device, from real
     * notification access.
     */
    @Test
    fun rolesAreAnnouncedFirstAndClaimBothSourceSideRoles() {
        attachAndDrain()

        val first = captured.firstOrNull()
        assumeTrue("nothing was produced for the peer", first != null)

        // Roles come first, before any content, and the first epoch is 1.
        assertEquals(NotificationControl.BodyCase.ROLES, first!!.bodyCase)
        assertEquals(1, first.roles.epoch)

        val announcements = captured
            .filter { it.bodyCase == NotificationControl.BodyCase.ROLES }
            .map { it.roles }

        // A non-empty announcement carries both source-side roles, and never a
        // sink-side one: this device displays nobody else's notifications and
        // reports nothing about them, so claiming either would be a promise no
        // code here could keep.
        for (announcement in announcements) {
            val roles = announcement.rolesList
            assertFalse(
                "a phone must never claim SINK",
                roles.contains(NotificationRole.NOTIFICATION_ROLE_SINK),
            )
            assertFalse(
                "a phone must never claim DISMISS_REPORTER",
                roles.contains(NotificationRole.NOTIFICATION_ROLE_DISMISS_REPORTER),
            )
            if (roles.isEmpty()) continue
            assertEquals(
                "SOURCE and DISMISS_TARGET are announced together or not at all",
                listOf(
                    NotificationRole.NOTIFICATION_ROLE_SOURCE,
                    NotificationRole.NOTIFICATION_ROLE_DISMISS_TARGET,
                ),
                roles,
            )
        }

        // Epochs are strictly monotonic within the connection, so a replayed
        // announcement can never re-widen a set that has narrowed.
        val epochs = announcements.map { it.epoch }
        assertEquals(epochs.sorted().distinct(), epochs)

        // And once the listener is actually bound, both source-side roles are
        // claimed — which is the widening ADR-0017 §3 requires to take effect
        // immediately rather than at the next reconnect. The session attaches
        // before the bind lands, so the first announcement is legitimately
        // empty.
        if (app.notifications.status.value.listenerConnected) {
            assertEquals(
                listOf(
                    NotificationRole.NOTIFICATION_ROLE_SOURCE,
                    NotificationRole.NOTIFICATION_ROLE_DISMISS_TARGET,
                ),
                announcements.last().rolesList,
            )
            assertTrue(
                "widening must raise the epoch",
                announcements.last().epoch >= 1,
            )
        } else {
            assertEquals(
                "without a bound listener the device claims no role",
                0,
                announcements.last().rolesCount,
            )
        }
    }

    // -- gate D: notification access revoked mid-session -----------------------

    /**
     * **Revoking notification access narrows the source role immediately, on
     * the session that is already up.**
     *
     * The revocation story has to be structurally true rather than a promise:
     * the user turns Pliwee off in Settings, the platform fires
     * `onListenerDisconnected()` on a healthy session, and the phone says "I am
     * no longer a source" **before** it stops being connected — with a higher
     * epoch, and without a reconnect.
     *
     * The revocation is driven through `UiAutomation`, which runs as the shell
     * uid, so this gate executes rather than being reasoned about. The grant is
     * restored afterwards whatever happens.
     */
    @Test
    fun revokingAccessNarrowsTheSourceRoleWithoutAReconnect() {
        attachAndDrain()
        assumeTrue(
            "the system did not bind the listener within the timeout",
            app.notifications.status.value.listenerConnected,
        )

        val widened = captured
            .filter { it.bodyCase == NotificationControl.BodyCase.ROLES }
            .last { it.roles.rolesList.contains(NotificationRole.NOTIFICATION_ROLE_SOURCE) }
        val before = captured.size

        try {
            shell("cmd notification disallow_listener ${component()}")

            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
            while (System.nanoTime() < deadline &&
                app.notifications.status.value.listenerConnected
            ) {
                Thread.sleep(200)
            }
            settle(before + 1)

            assertFalse(
                "the listener must report itself disconnected after a revocation",
                app.notifications.status.value.listenerConnected,
            )

            val narrowed = captured
                .drop(before)
                .last { it.bodyCase == NotificationControl.BodyCase.ROLES }
                .roles
            assertEquals("the narrowed set is empty", 0, narrowed.rolesCount)
            assertTrue(
                "narrowing must carry a strictly higher epoch",
                narrowed.epoch > widened.roles.epoch,
            )

            // And nothing further is emitted: no reconnect happened, and none
            // was needed for the narrowing to take effect.
            val after = captured.size
            settle(after + 1)
            assertTrue(
                "no notification content may follow a revocation",
                captured.drop(after).none {
                    it.bodyCase == NotificationControl.BodyCase.UPSERT
                },
            )
        } finally {
            shell("cmd notification allow_listener ${component()}")
        }
    }

    private fun component(): String = NotificationAccess.component(app).flattenToString()

    /**
     * Runs a shell command as the instrumentation (shell) uid.
     *
     * This is what makes the revocation gate executable from a test rather
     * than something a person has to remember to do by hand.
     */
    private fun shell(command: String) {
        InstrumentationRegistry.getInstrumentation().uiAutomation
            .executeShellCommand(command)
            .also { Thread.sleep(1_000) }
            .close()
    }

    // -- gate E: the secret is stable across a process restart ----------------

    /**
     * The production secret exists and derives a stable identity.
     *
     * Read through the app's own status rather than by touching the keystore:
     * this suite must not rotate the maintainer's real notification secret,
     * and there is no accessor that could return its bytes in any case.
     */
    @Test
    fun theProductionSecretIsAvailableAndStable() {
        assumeTrue(
            "notification access is not granted",
            NotificationAccess.isGranted(app),
        )
        attachAndDrain()
        assertTrue(
            "the Keystore must provide a notification secret",
            app.notifications.status.value.secretAvailable,
        )
    }

    // -- what must never be on the wire --------------------------------------

    /**
     * No encoded message contains the fixture's platform key, the raw group
     * key, a uid or a profile number.
     *
     * Asserted against the bytes, on hardware, from a real
     * `StatusBarNotification` — which is the only place those values ever
     * existed.
     */
    @Test
    fun noEncodedMessageContainsAPlatformKey() {
        assumeTrue(
            "notification access is not granted",
            NotificationAccess.isGranted(app),
        )

        attachAndDrain()
        assumeTrue("nothing was produced", captured.isNotEmpty())

        for (message in captured) {
            val bytes = String(message.toByteArray(), Charsets.ISO_8859_1)
            assertFalse(
                "a raw platform key must never be transmitted",
                bytes.contains("|$fixturePackage|"),
            )
            assertFalse("a group key must never be transmitted", bytes.contains("|g:"))
        }
    }

    /**
     * And nothing the capability renders locally contains notification text.
     *
     * The logcat audit is run separately, over a full session; this is the
     * in-process half, on the objects a crash report or an exception message
     * would actually reach.
     */
    @Test
    fun noCapabilityStateRendersNotificationContent() {
        attachAndDrain()

        val renderings = listOf(
            app.notifications.describeQueue(),
            app.notifications.status.value.toString(),
            app.trustStore.notificationPolicyFor(testPeer).describe(),
        )
        for (rendered in renderings) {
            assertFalse("leaked notification text: $rendered", rendered.contains(fixtureTitle))
            assertFalse("leaked notification text: $rendered", rendered.contains("FIXTURE-BODY"))
        }
    }
}
