package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.PliweeApp
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.notifications.NotificationApp
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.notifications.NotificationSource
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.MainActions
import io.github.yurisismotto.pliwee.ui.MainUiState

/**
 * The state and actions the consent screens are driven with, in one place.
 *
 * Every screen under test is a pure function of [MainUiState], which is what
 * makes these tests deterministic: no daemon, no session, no package manager
 * and no permission dialog. What is exercised is the thing the wave is
 * actually accountable for — whether a switch is off by default, whether a
 * status can claim to be mirroring while a gate is shut, and whether anything
 * shares an application nobody named.
 */
object NotificationUiFixtures {

    const val PEER_HEX = "7e637b4e937b77320123456789abcdef0123456789abcdef0123456789abcdef"
    const val OWN_PACKAGE = "io.github.yurisismotto.pliwee"

    fun fingerprint(): Fingerprint = Fingerprint.fromHex(PEER_HEX)!!

    fun peer(
        granted: Boolean = false,
        policy: NotificationPolicy = NotificationPolicy(),
        otherGrants: Set<String> = setOf("battery.v1", "files.v1"),
    ): TrustStore.TrustedPeer = TrustStore.TrustedPeer(
        deviceId = "fedora-desktop",
        deviceName = "fedora",
        fingerprint = fingerprint(),
        pairedAtUnix = 1_700_000_000,
        grantedCapabilities = if (granted) otherGrants + "notifications.v1" else otherGrants,
        addressHints = emptyList(),
        notificationPolicy = policy,
    )

    /** A session in which this phone is sourcing and the computer is a sink. */
    fun sourcingStatus(
        peerIsSink: Boolean = true,
        peerIsDismissReporter: Boolean = true,
        dismissRequests: Int = 0,
        dismissesPerformed: Int = 0,
    ): NotificationSource.Status =
        NotificationSource.Status(
            listenerConnected = true,
            accessGranted = true,
            secretAvailable = true,
            sessions = 1,
            peers = mapOf(
                PEER_HEX to NotificationSource.PeerStatus(
                    localIsSource = true,
                    localIsDismissTarget = true,
                    localEpoch = 1,
                    peerIsSink = peerIsSink,
                    peerIsDismissReporter = peerIsDismissReporter,
                    peerEpoch = 2,
                    dismissRequests = dismissRequests,
                    dismissesPerformed = dismissesPerformed,
                ),
            ),
        )

    /** No session, and therefore no bound listener. The idle state. */
    fun idleStatus(accessGranted: Boolean = true): NotificationSource.Status =
        NotificationSource.Status(
            listenerConnected = false,
            accessGranted = accessGranted,
            secretAvailable = true,
        )

    fun state(
        peer: TrustStore.TrustedPeer,
        accessGranted: Boolean = true,
        status: NotificationSource.Status = sourcingStatus(),
        hasWorkProfile: Boolean = false,
        /**
         * Which computer the person chose, as P1's explicit-target model
         * requires (`PeerTarget`).
         *
         * `null` is the honest default here and not a shortcut: this fixture
         * builds a state with exactly **one** trusted peer, and one candidate
         * resolves to `OnlyTrustedPeer` whether or not a choice was ever made.
         * So the consent screens under test get a target without this fixture
         * asserting anything about routing, which is `PeerTargetTest`'s
         * subject and not theirs. A parameter rather than a hard-coded `null`
         * so a future multi-peer consent test can state its own choice.
         */
        selectedPeerHex: String? = null,
        /**
         * The rows the device list draws: trusted computers, plus revoked ones
         * the person has not yet removed.
         *
         * Defaults to the same single peer as [peers], which is the truth for
         * every consent test here — the fixture builds one *trusted* computer,
         * and a trusted computer is both trusted and listed.
         *
         * It is a real value rather than an empty placeholder because the
         * screens depend on it: `MainUiState.peerByHex` resolves against the
         * listed set, so a fixture that left this empty would hand every
         * consent screen a null peer and render "This device is no longer
         * paired" instead of the thing under test. A parameter so a future
         * test can state a revoked row of its own.
         */
        listedPeers: List<TrustStore.TrustedPeer> = listOf(peer),
    ): MainUiState = MainUiState(
        ownDeviceName = "Tablet",
        ownFingerprint = "0000 0000 0000 0000",
        keyBackingDescription = "Key stored in the hardware-backed keystore",
        connection = PliweeApp.ConnectionState.Connected("fedora", "7E63 7B4E 937B 7732"),
        peers = listOf(peer),
        listedPeers = listedPeers,
        selectedPeerHex = selectedPeerHex,
        offers = emptyList(),
        transfers = emptyList(),
        pendingClips = emptyList(),
        clipboardDeliveries = emptyMap(),
        liveSession = null,
        remoteBatteryPercent = null,
        notificationAccessGranted = accessGranted,
        notifications = status,
        hasWorkProfile = hasWorkProfile,
    )

    /** Records everything a screen asked for, so a test can assert on it. */
    class Recorder {
        val grants = mutableListOf<Boolean>()
        val policies = mutableListOf<NotificationPolicy>()
        var settingsOpened = 0
        var appsLoaded = 0

        /**
         * Destructive trust actions a screen asked for.
         *
         * Recorded rather than swallowed by a no-op. No consent screen should
         * ever ask for either of these, and a recorder is what lets a test say
         * so — "the notification screen withdrew trust from a computer" is a
         * failure worth being able to catch, and an empty lambda would hide
         * it. They replace the single `onForget`, which deleted the record;
         * the two now mirror the real lifecycle (revoke, then remove from the
         * list).
         */
        val revoked = mutableListOf<TrustStore.TrustedPeer>()
        val removedFromList = mutableListOf<TrustStore.TrustedPeer>()

        /** What the picker is handed. Deliberately not the real device's. */
        var apps: List<NotificationApp> = emptyList()

        fun actions(): MainActions = MainActions(
            onPair = {},
            onConnect = {},
            onDisconnect = {},
            onRevoke = { peer -> revoked += peer },
            onRemoveFromList = { peer -> removedFromList += peer },
            onSetFilesGrant = { _, _ -> },
            onSetClipboardGrant = { _, _ -> },
            onSetBatteryGrant = { _, _ -> },
            onSetClipboardPolicy = { _, _ -> },
            onSetNotificationsGrant = { _, granted -> grants += granted },
            onSetNotificationPolicy = { _, policy -> policies += policy },
            onOpenNotificationAccess = { settingsOpened += 1 },
            onOpenPrivacyPolicy = {},
            loadNotificationApps = {
                appsLoaded += 1
                apps
            },
            loadAppIcon = { null },
            onSendClipboard = {},
            onApplyClip = {},
            onDismissClip = {},
            onRespondToOffer = { _, _ -> },
            onCancelTransfer = {},
            onOpenTransfer = {},
            onRefreshOpenTargets = {},
            onPickFileFor = {},
            readClipboardPreview = { null },
        )
    }

    fun app(
        packageName: String,
        label: String,
        allowed: Boolean = false,
        notifying: Boolean = false,
    ) = NotificationApp(
        packageName = packageName,
        label = label,
        launchable = true,
        notifying = notifying,
        allowed = allowed,
    )
}
