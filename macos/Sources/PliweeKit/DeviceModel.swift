// What the app shows about each device, decided on plain data.
//
// The rules are the GTK Quick Panel's (`desktop/gui/src/panel/model.rs`),
// carried over rather than re-invented: being paired is durable, having a
// session is momentary, and the last battery reading is neither. Nothing
// here grants anything — the agent re-checks every grant when a request
// arrives — so working from a two-second-old poll is safe.

import Foundation

/// The capability identifiers this app knows how to present.
public enum Capability {
    public static let battery = "battery.v1"
    public static let files = "files.v1"
    public static let clipboard = "clipboard.v1"
    public static let notifications = "notifications.v1"

    /// Display order, and the human name of each.
    public static let known: [(id: String, title: String)] = [
        (files, "Files"),
        (clipboard, "Clipboard"),
        (battery, "Battery"),
        (notifications, "Notifications"),
    ]

    public static func title(of id: String) -> String {
        known.first { $0.id == id }?.title ?? id
    }
}

/// The state of the link to a device.
public enum PeerLink: Equatable, Sendable {
    case connected
    /// A session exists but nothing has arrived for long enough that what it
    /// last said is history.
    case stale
    case offline

    public init(_ state: DeviceState) {
        switch state {
        case .connected: self = .connected
        case .stale: self = .stale
        case .disconnected, .revoked, .unknown: self = .offline
        }
    }

    public var label: String {
        switch self {
        case .connected: return "Connected"
        case .stale: return "Not responding"
        case .offline: return "Offline"
        }
    }

    /// Whether an action that needs a live session can run.
    public var isLive: Bool { self == .connected }
}

/// A device's battery, as this Mac is entitled to describe it.
///
/// `absent` is a real answer: `battery.v1` says "nothing truthful" by sending
/// nothing, so a missing reading stays missing all the way to the screen and
/// is never drawn as 0%.
public enum Battery: Equatable, Sendable {
    case absent
    case present(percent: UInt32, charging: String, stale: Bool)

    public init(_ report: BatteryReport?) {
        guard let report else { self = .absent; return }
        self = .present(percent: report.percentage, charging: report.chargingState, stale: report.stale)
    }

    public var label: String? {
        guard case let .present(percent, charging, stale) = self else { return nil }
        var parts = ["\(percent)%"]
        switch charging {
        case "charging", "Charging": parts.append("Charging")
        case "full", "Full": parts.append("Full")
        case "not_charging", "NotCharging": parts.append("Not charging")
        default: break
        }
        if stale { parts.append("last reported") }
        return parts.joined(separator: " · ")
    }
}

/// Whether one capability is allowed for a device, and live in its session.
public struct CapabilityState: Equatable, Sendable {
    /// The user granted it. Durable.
    public let granted: Bool
    /// The current session negotiated it. Momentary.
    public let live: Bool
}

/// One paired device, ready to draw.
public struct PeerCard: Equatable, Sendable, Identifiable {
    /// Full fingerprint hex: the destination of every action.
    public let fingerprint: String
    public let fingerprintShort: String
    /// The device id. Empty for a revoked device, whose record was cleared.
    public let deviceId: String
    public let name: String
    public let platform: String
    public let link: PeerLink
    public let battery: Battery
    public let revoked: Bool
    public let lastSeenSecsAgo: UInt64?
    public let grantedCapabilities: [String]
    public let negotiatedCapabilities: [String]

    public var id: String { fingerprint }

    public func capability(_ id: String) -> CapabilityState {
        CapabilityState(
            granted: grantedCapabilities.contains(id),
            live: link.isLive && negotiatedCapabilities.contains(id)
        )
    }

    public var files: CapabilityState { capability(Capability.files) }
    public var clipboard: CapabilityState { capability(Capability.clipboard) }

    /// The line under a device's name.
    public var subtitle: String {
        var parts = [link.label]
        if let battery = battery.label { parts.append(battery) }
        return parts.joined(separator: " · ")
    }

    /// A short human description of the platform the device stated.
    public var platformLabel: String? {
        switch platform {
        case "android": return "Android"
        case "linux": return "Linux"
        default: return nil
        }
    }
}

public enum DeviceDirectory {
    /// The devices to show, from one status report.
    ///
    /// Negotiated capabilities come from the device's live connection, matched
    /// by device id; a device without a live session has none. Revoked devices
    /// are included and marked, so the Devices page can show and remove them;
    /// `trusted` drops them for every action surface.
    public static func cards(from status: StatusReport) -> [PeerCard] {
        status.devices.map { device in
            let session = status.connections.first {
                $0.deviceId == device.deviceId && !device.deviceId.isEmpty
            }
            return PeerCard(
                fingerprint: device.fingerprint,
                fingerprintShort: device.fingerprintShort,
                deviceId: device.deviceId,
                name: device.deviceName.isEmpty ? "Unnamed device" : device.deviceName,
                platform: device.platform,
                link: device.revoked ? .offline : PeerLink(device.state),
                battery: Battery(device.battery),
                revoked: device.revoked,
                lastSeenSecsAgo: device.lastSeenSecsAgo,
                grantedCapabilities: device.grantedCapabilities,
                negotiatedCapabilities: session?.negotiatedCapabilities ?? []
            )
        }
        .sorted { ($0.revoked ? 1 : 0, $0.name.lowercased(), $0.fingerprint) < ($1.revoked ? 1 : 0, $1.name.lowercased(), $1.fingerprint) }
    }

    /// Paired and not revoked: the devices an action can target.
    public static func trusted(from status: StatusReport) -> [PeerCard] {
        cards(from: status).filter { !$0.revoked }
    }
}
