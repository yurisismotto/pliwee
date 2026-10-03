import Foundation
import Testing
@testable import PliweeKit

/// A status report from the shared fixture, so model tests run on the same
/// data the protocol tests decode.
func fixtureStatus() throws -> StatusReport {
    guard case let .status(status) = try ControlCoding.response(from: try Fixture.data("responses", "status")) else {
        throw ControlError.malformed("status")
    }
    return status
}

func fixtureClipboard() throws -> ClipboardStatusReport {
    guard case let .clipboard(report) = try ControlCoding.response(from: try Fixture.data("responses", "clipboard")) else {
        throw ControlError.malformed("clipboard")
    }
    return report
}

func card(
    name: String = "Phone",
    link: PeerLink = .connected,
    granted: [String] = [Capability.files, Capability.clipboard],
    negotiated: [String] = [Capability.files, Capability.clipboard],
    revoked: Bool = false,
    deviceId: String = "0123456789abcdef0123456789abcdef",
    short: String = "A1B2 C3D4 E5F6 0718"
) -> PeerCard {
    PeerCard(
        fingerprint: String(repeating: "ab", count: 32),
        fingerprintShort: short,
        deviceId: deviceId,
        name: name,
        platform: "android",
        link: link,
        battery: .absent,
        revoked: revoked,
        lastSeenSecsAgo: nil,
        grantedCapabilities: granted,
        negotiatedCapabilities: negotiated
    )
}

@Suite struct DeviceModelTests {
    @Test func cardsComeFromTheStatusReportWithLiveCapabilitiesFromTheSession() throws {
        let cards = DeviceDirectory.cards(from: try fixtureStatus())
        #expect(cards.count == 2)
        let phone = try #require(cards.first)
        #expect(phone.name == "Galaxy S25")
        #expect(phone.link == .connected)
        #expect(phone.files == CapabilityState(granted: true, live: true))
        #expect(phone.clipboard == CapabilityState(granted: false, live: false))
        #expect(phone.battery == .present(percent: 81, charging: "charging", stale: false))
        #expect(phone.platformLabel == "Android")
    }

    @Test func revokedDevicesAreListedLastAndNeverTrusted() throws {
        let status = try fixtureStatus()
        let cards = DeviceDirectory.cards(from: status)
        #expect(cards.last?.revoked == true)
        #expect(cards.last?.name == "Unnamed device", "a revoked record has its name cleared")
        #expect(DeviceDirectory.trusted(from: status).allSatisfy { !$0.revoked })
        #expect(DeviceDirectory.trusted(from: status).count == 1)
    }

    @Test func aCapabilityIsLiveOnlyOnAConnectedLink() {
        let stale = card(link: .stale)
        #expect(stale.files == CapabilityState(granted: true, live: false))
    }

    @Test func noBatteryIsNeverZeroPercent() {
        #expect(Battery(nil) == .absent)
        #expect(Battery.absent.label == nil)
        let reading = BatteryReport(percentage: 0, chargingState: "Discharging", ageSecs: 1, stale: false)
        #expect(Battery(reading).label == "0%", "a real 0% is still shown")
    }

    @Test func batteryLabelsUseTheAgentsWords() {
        #expect(Battery.present(percent: 55, charging: "Charging", stale: false).label == "55% · Charging")
        #expect(Battery.present(percent: 80, charging: "NotCharging", stale: false).label == "80% · Not charging")
        #expect(Battery.present(percent: 40, charging: "Discharging", stale: true).label == "40% · last reported")
    }

    @Test func linkStatesMapFromDeviceStates() {
        #expect(PeerLink(.connected) == .connected)
        #expect(PeerLink(.stale) == .stale)
        #expect(PeerLink(.disconnected) == .offline)
        #expect(PeerLink(.unknown) == .offline, "an unknown state is never treated as live")
    }
}

@Suite struct ActionModelTests {
    let running = ServiceHealth.running(connected: 1, paired: 1)

    @Test func aReadyActionCarriesTheFullFingerprintNotTheName() {
        let peer = card()
        #expect(Actions.sendFile(health: running, peer: peer) == .ready(fingerprint: peer.fingerprint, peerName: "Phone"))
    }

    @Test func nothingRunsWhileTheServiceIsDown() {
        let action = Actions.sendFile(health: .off, peer: card())
        #expect(action.reason == ServiceHealth.off.headline)
    }

    @Test func fileSendingNeedsAGrantAndANegotiatedSession() {
        #expect(Actions.sendFile(health: running, peer: card(granted: [])).reason == "Files are not enabled for Phone.")
        #expect(Actions.sendFile(health: running, peer: card(negotiated: [])).reason
            == "This session with Phone has not negotiated file transfer.")
        #expect(Actions.sendFile(health: running, peer: card(link: .offline)).reason == "Phone is offline.")
        #expect(Actions.sendFile(health: running, peer: card(link: .stale)).reason == "Phone is not responding.")
        #expect(Actions.sendFile(health: running, peer: card(revoked: true)).reason == "Phone is no longer paired.")
    }

    @Test func clipboardSendingFollowsThePerPeerPolicy() throws {
        let report = try fixtureClipboard()
        let peer = card(name: "Galaxy S25")
        #expect(Actions.sendClipboard(health: running, peer: peer, report: report).isReady)
        #expect(Actions.sendClipboard(health: running, peer: peer, report: nil).reason == "Waiting for the Pliwee service…")
        let unknown = card(name: "Other", deviceId: "ffff")
        #expect(Actions.sendClipboard(health: running, peer: unknown, report: report).reason
            == "The Pliwee service has no clipboard policy for Other.")
    }

    @Test func aPolicyRowIsMatchedOnDeviceIdAndShortFingerprintTogether() throws {
        let report = try fixtureClipboard()
        #expect(Actions.clipboardPolicy(in: report, for: card()) != nil)
        #expect(Actions.clipboardPolicy(in: report, for: card(short: "FFFF FFFF FFFF FFFF")) == nil)
        #expect(Actions.clipboardPolicy(in: report, for: card(deviceId: "other")) == nil)
    }
}

@Suite struct ServiceHealthTests {
    @Test func anAnsweringAgentIsRunningWhateverTheRegistrationSays() throws {
        let status = try fixtureStatus()
        for registration in [AgentRegistration.notRegistered, .enabled, .requiresApproval, .notFound] {
            #expect(ServiceHealth.resolve(probe: .answered(status), registration: registration, secondsSinceEnabled: nil)
                == .running(connected: 1, paired: 1))
        }
    }

    @Test func silenceIsExplainedByTheRegistration() {
        let failed = Probe.failed("connection refused")
        #expect(ServiceHealth.resolve(probe: failed, registration: .notRegistered, secondsSinceEnabled: nil) == .off)
        #expect(ServiceHealth.resolve(probe: failed, registration: .requiresApproval, secondsSinceEnabled: nil) == .needsApproval)
        #expect(ServiceHealth.resolve(probe: failed, registration: .notFound, secondsSinceEnabled: nil)
            == .unreachable("connection refused"))
    }

    @Test func aJustEnabledAgentIsStartingUntilTheGraceRunsOut() {
        let failed = Probe.failed("no such file")
        #expect(ServiceHealth.resolve(probe: failed, registration: .enabled, secondsSinceEnabled: 2) == .starting)
        #expect(ServiceHealth.resolve(probe: failed, registration: .enabled, secondsSinceEnabled: ServiceHealth.startupGrace + 1)
            == .notResponding("no such file"))
        #expect(ServiceHealth.resolve(probe: failed, registration: .enabled, secondsSinceEnabled: nil)
            == .notResponding("no such file"), "an agent enabled before this app started has had its grace")
    }

    @Test func theMenuTitleSaysConnectedOnlyWithALiveDevice() {
        #expect(ServiceHealth.running(connected: 1, paired: 2).title == "Connected")
        #expect(ServiceHealth.running(connected: 0, paired: 2).title == "Disconnected")
        #expect(ServiceHealth.starting.title == "Starting…")
        #expect(ServiceHealth.off.title == "Service off")
    }

    @Test func headlinesAreSentences() {
        for health in [ServiceHealth.running(connected: 0, paired: 0), .running(connected: 0, paired: 1),
                       .running(connected: 2, paired: 3), .starting, .off, .needsApproval,
                       .notResponding("x"), .unreachable("x")] {
            #expect(health.headline.hasSuffix(".") || health.headline.hasSuffix("…"), "\(health)")
        }
    }
}

@Suite struct RuntimePathsTests {
    // The same literals `desktop/platform-macos/src/paths.rs` asserts. If one
    // side moves the socket and the other does not, the app cannot find the
    // agent; these two tests are what notice.
    @Test func pathsMatchTheAgentsLineForLine() {
        let p = RuntimePaths(home: URL(fileURLWithPath: "/Users/ana", isDirectory: true))
        #expect(p.dataDirectory.path == "/Users/ana/Library/Application Support/Pliwee")
        #expect(p.controlSocket.path == "/Users/ana/Library/Application Support/Pliwee/run/control.sock")
        #expect(p.logFile.path == "/Users/ana/Library/Logs/Pliwee/pliweed.log")
        #expect(p.downloadsDirectory.path == "/Users/ana/Downloads/Pliwee")
    }

    @Test func homeComesFromTheEnvironmentWhenAbsolute() {
        #expect(RuntimePaths.forCurrentUser(environment: ["HOME": "/Users/bo"]).home.path == "/Users/bo")
        #expect(RuntimePaths.forCurrentUser(environment: ["HOME": "relative"]).home.path != "relative")
    }
}

@Suite struct FormattingTests {
    @Test func fingerprintsAreGroupedInFours() {
        #expect(Format.fingerprint("a1b2c3d4e5f6") == "A1B2 C3D4 E5F6")
        #expect(Format.fingerprint("abcde") == "ABCD E")
    }

    @Test func pairingOutcomesAreSentences() {
        #expect(Format.pairingOutcome(status: "paired", detail: "A1B2") == "Paired. Fingerprint A1B2.")
        #expect(Format.pairingOutcome(status: "expired", detail: "").hasPrefix("The pairing code expired"))
    }
}

@Suite struct DesignTokensTests {
    static let tokensURL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("docs/design/tokens.json")

    func tokens() throws -> [String: Any] {
        try #require(JSONSerialization.jsonObject(with: Data(contentsOf: Self.tokensURL)) as? [String: Any])
    }

    @Test func brandHuesMatchTheCanonicalTokens() throws {
        let brand = try #require(try tokens()["brand"] as? [String: String])
        #expect(brand["cyan"] == DesignTokens.Brand.cyan)
        #expect(brand["blue"] == DesignTokens.Brand.blue)
        #expect(brand["violet"] == DesignTokens.Brand.violet)
        #expect(brand["dark"] == DesignTokens.Brand.dark)
        #expect(brand["surface"] == DesignTokens.Brand.surface)
    }

    @Test func textCorrectionsMatchTheCanonicalTokens() throws {
        let light = try #require(try tokens()["on_light"] as? [String: String])
        let dark = try #require(try tokens()["on_dark"] as? [String: String])
        let table: [(String, DesignTokens.Pair)] = [
            ("cyan", DesignTokens.Text.cyan), ("blue", DesignTokens.Text.blue),
            ("violet", DesignTokens.Text.violet), ("amber", DesignTokens.Text.amber),
            ("red", DesignTokens.Text.red),
        ]
        for (name, pair) in table {
            #expect(light[name] == pair.light, "on_light.\(name)")
            #expect(dark[name] == pair.dark, "on_dark.\(name)")
        }
    }

    @Test func statusColoursMatchTheCanonicalTokens() throws {
        let status = try #require(try tokens()["status"] as? [String: Any])
        let table: [(String, DesignTokens.Pair)] = [
            ("connected", DesignTokens.Status.connected), ("available", DesignTokens.Status.available),
            ("transferring", DesignTokens.Status.transferring), ("success", DesignTokens.Status.success),
            ("warning", DesignTokens.Status.warning), ("error", DesignTokens.Status.error),
            ("stale", DesignTokens.Status.stale),
        ]
        for (name, pair) in table {
            let entry = try #require(status[name] as? [String: String], "status.\(name)")
            #expect(entry["light"] == pair.light, "status.\(name).light")
            #expect(entry["dark"] == pair.dark, "status.\(name).dark")
        }
    }

    @Test func hexParsing() {
        let rgb = DesignTokens.rgb("#4F6BFF")
        #expect(rgb?.red == Double(0x4F) / 255)
        #expect(DesignTokens.rgb("nope") == nil)
    }
}

@Suite struct SingleInstanceTests {
    let t0 = Date(timeIntervalSince1970: 1_000)

    @Test func aLoneInstanceKeepsRunning() {
        #expect(SingleInstance.instanceToYieldTo(selfPID: 10, running: [.init(pid: 10, launched: t0)]) == nil)
        #expect(SingleInstance.instanceToYieldTo(selfPID: 10, running: []) == nil)
    }

    @Test func aNewcomerYieldsToTheRunningInstance() {
        let running = [SingleInstance.Instance(pid: 10, launched: t0),
                       .init(pid: 20, launched: t0.addingTimeInterval(60))]
        #expect(SingleInstance.instanceToYieldTo(selfPID: 20, running: running)?.pid == 10)
    }

    @Test func withSeveralOthersTheEarliestLaunchedIsKept() {
        let running = [SingleInstance.Instance(pid: 30, launched: t0.addingTimeInterval(30)),
                       .init(pid: 10, launched: t0),
                       .init(pid: 40, launched: t0.addingTimeInterval(90))]
        #expect(SingleInstance.instanceToYieldTo(selfPID: 40, running: running)?.pid == 10)
        #expect(SingleInstance.instanceToYieldTo(selfPID: 30, running: running)?.pid == 10)
    }

    @Test func anInstanceWithoutALaunchDateIsNotPreferred() {
        let running = [SingleInstance.Instance(pid: 10, launched: nil),
                       .init(pid: 20, launched: t0),
                       .init(pid: 30, launched: t0.addingTimeInterval(5))]
        #expect(SingleInstance.instanceToYieldTo(selfPID: 30, running: running)?.pid == 20)
    }
}
