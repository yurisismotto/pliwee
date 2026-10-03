// The background agent's registration, through ServiceManagement.
//
// `pliweed` ships inside the bundle, at `Contents/MacOS/pliweed`, and is
// described by `Contents/Library/LaunchAgents/<label>.plist`. Registering
// that plist with `SMAppService.agent` makes it a per-user `launchd` agent:
// started now and at every login, restarted if it fails, listed under
// System Settings › General › Login Items, and stopped and removed by
// unregistering. No root, no installer, nothing outside the bundle.
//
// This is the macOS counterpart of `systemctl --user enable --now
// pliweed.service`, and it has the same default: nothing runs and nothing
// listens on the network until the person turns it on.

import Foundation
import PliweeKit
import ServiceManagement

@MainActor
final class AgentService {
    /// Must match `Label` in `Resources/LaunchAgents/` and
    /// `pliwee_macos::LAUNCHD_LABEL`; `scripts/build-app.sh` checks both.
    static let label = "io.github.yurisismotto.pliwee.daemon"
    static let plistName = "\(label).plist"

    private let agent = SMAppService.agent(plistName: AgentService.plistName)

    /// Whether this process is running from a bundle that carries the
    /// agent: `Contents/Library/LaunchAgents/<label>.plist`. `swift run`
    /// does not, and cannot register anything.
    let bundlesAgent: Bool = FileManager.default.fileExists(
        atPath: Bundle.main.bundleURL
            .appendingPathComponent("Contents/Library/LaunchAgents/\(AgentService.plistName)").path
    )

    /// The agent's registration, as ServiceManagement reports it right now.
    var registration: AgentRegistration {
        Self.map(agent.status, bundled: bundlesAgent)
    }

    /// Turns the background service on or off.
    func setEnabled(_ enabled: Bool) throws {
        if enabled {
            try agent.register()
        } else {
            try agent.unregister()
        }
    }

    /// Whether `Pliwee.app` itself opens at login, for its menu-bar item.
    ///
    /// Separate from the agent: the service can run with no menu-bar item,
    /// exactly as `pliweed` runs on Linux with no window open.
    var openAtLogin: AgentRegistration {
        Self.map(SMAppService.mainApp.status, bundled: bundlesAgent)
    }

    func setOpenAtLogin(_ enabled: Bool) throws {
        if enabled {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
    }

    func openLoginItemsSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }

    /// `.notFound` has two meanings in practice. Measured on macOS 27: a
    /// bundle that has never registered anything reads `.notFound` — Background
    /// Task Management logs "record not found" for it — and becomes
    /// `.notRegistered` after its first `unregister`. So `.notFound` from a
    /// bundle that does carry the agent means "not registered yet", and only
    /// from one that does not (`swift run`) does it mean "cannot be managed".
    private static func map(_ status: SMAppService.Status, bundled: Bool) -> AgentRegistration {
        switch status {
        case .enabled: return .enabled
        case .requiresApproval: return .requiresApproval
        case .notRegistered: return .notRegistered
        case .notFound: return bundled ? .notRegistered : .notFound
        @unknown default: return bundled ? .notRegistered : .notFound
        }
    }
}
