// How the Pliwee service stands, from what can actually be observed.
//
// Two observations go in: whether the control socket answered `status`, and
// what ServiceManagement says about the background agent's registration.
// One answer comes out, and it is the only thing the menu bar and the
// Overview page say about the service. Neither observation alone is enough:
// a registered agent may not be running yet, and a running agent may have
// been started from a terminal and never registered at all.

import Foundation

/// The background agent's registration, as `SMAppService` reports it.
///
/// Mirrored here, rather than using `SMAppService.Status` directly, so the
/// rules below can be tested without ServiceManagement and without a bundle.
public enum AgentRegistration: Equatable, Sendable {
    /// Not registered: the service is off, by the user's choice or because
    /// it has never been turned on.
    case notRegistered
    /// Registered and allowed to run.
    case enabled
    /// Registered, and waiting for the user to allow it in System Settings →
    /// General → Login Items.
    case requiresApproval
    /// The agent's plist is not in the app bundle — a development build run
    /// outside `Pliwee.app`.
    case notFound
}

/// What one attempt to reach the agent found.
public enum Probe: Equatable, Sendable {
    case answered(StatusReport)
    case failed(String)
}

public enum ServiceHealth: Equatable, Sendable {
    /// The agent answered.
    case running(connected: Int, paired: Int)
    /// Registered and enabled, not answering yet, within the start-up grace.
    case starting
    /// Not registered and not answering: the user has not turned it on.
    case off
    /// Registered, waiting for approval in Login Items.
    case needsApproval
    /// Registered and enabled, and still not answering after the grace.
    case notResponding(String)
    /// Not answering, and there is no registration to explain it either way
    /// — a development build, or an agent the user runs by hand.
    case unreachable(String)

    /// How long a just-enabled agent may take to open its socket before the
    /// silence is reported as a problem. `launchd` starts it at once; the
    /// first start also creates an identity, which can wait on a keychain
    /// prompt.
    public static let startupGrace: TimeInterval = 20

    public static func resolve(
        probe: Probe,
        registration: AgentRegistration,
        secondsSinceEnabled: TimeInterval?
    ) -> ServiceHealth {
        if case let .answered(status) = probe {
            let paired = status.devices.filter { !$0.revoked }.count
            let connected = status.devices.filter { !$0.revoked && $0.state == .connected }.count
            return .running(connected: connected, paired: paired)
        }
        guard case let .failed(reason) = probe else { return .unreachable("") }
        switch registration {
        case .notRegistered:
            return .off
        case .requiresApproval:
            return .needsApproval
        case .enabled:
            if let since = secondsSinceEnabled, since < startupGrace {
                return .starting
            }
            return .notResponding(reason)
        case .notFound:
            return .unreachable(reason)
        }
    }

    public var isRunning: Bool {
        if case .running = self { return true }
        return false
    }

    /// One or two words, for the menu's status row.
    public var title: String {
        switch self {
        case let .running(connected, _): return connected > 0 ? "Connected" : "Disconnected"
        case .starting: return "Starting…"
        case .off: return "Service off"
        case .needsApproval: return "Needs approval"
        case .notResponding: return "Not responding"
        case .unreachable: return "Not running"
        }
    }

    /// A full sentence: the reason an action is unavailable, and the
    /// explanation under the status.
    public var headline: String {
        switch self {
        case let .running(connected, paired):
            if paired == 0 { return "Pliwee is running. No device is paired yet." }
            if connected == 0 {
                return paired == 1
                    ? "Pliwee is running. Your device is not connected."
                    : "Pliwee is running. None of your \(paired) devices is connected."
            }
            return connected == 1
                ? "Pliwee is running. 1 device connected."
                : "Pliwee is running. \(connected) devices connected."
        case .starting:
            return "The Pliwee service is starting…"
        case .off:
            return "The Pliwee service is off. Turn it on to pair and connect devices."
        case .needsApproval:
            return "Allow Pliwee in System Settings › General › Login Items to start the service."
        case .notResponding:
            return "The Pliwee service is turned on but is not responding. Its log may say why."
        case .unreachable:
            return "The Pliwee service is not running."
        }
    }

    /// The SF Symbol for the status row.
    public var symbol: String {
        switch self {
        case let .running(connected, _): return connected > 0 ? "checkmark.circle.fill" : "circle.dashed"
        case .starting: return "hourglass"
        case .off: return "pause.circle"
        case .needsApproval: return "exclamationmark.circle"
        case .notResponding, .unreachable: return "exclamationmark.triangle.fill"
        }
    }
}
