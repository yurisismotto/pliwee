// The local control protocol, as `Pliwee.app` speaks it.
//
// A transcription of `desktop/control/src/lib.rs` — newline-delimited JSON,
// one request per connection — and nothing more. The Rust front ends share
// those types with the agent; this file cannot, so it is checked instead
// against `desktop/control/tests/fixtures/control-protocol.json`, which the
// Rust test suite generates from the real types. A change on either side
// that the other has not made fails `swift test` or `cargo test`.
//
// Like the Rust types it mirrors, nothing here can hold clipboard content, a
// notification's text, a byte of a file or any key material: the control
// socket never carries them.

import Foundation

// MARK: - Requests

/// A per-peer clipboard policy flag (`ClipboardFlag`).
public enum ClipboardFlag: String, Sendable, CaseIterable {
    case send
    case receive
    case autoSend = "auto_send"
    case autoReceive = "auto_receive"
}

/// A per-peer notification setting (`NotificationSetting`).
public enum NotificationSetting: Equatable, Sendable {
    case mirror(enabled: Bool)
    case whenLocked(policy: String)
    case dismissSync(enabled: Bool)

    var jsonObject: [String: Any] {
        switch self {
        case let .mirror(enabled): return ["setting": "mirror", "enabled": enabled]
        case let .whenLocked(policy): return ["setting": "when_locked", "policy": policy]
        case let .dismissSync(enabled): return ["setting": "dismiss_sync", "enabled": enabled]
        }
    }
}

/// A request to the agent (`Request`).
///
/// Every `device` is a selector the agent resolves; this app always passes a
/// full fingerprint, never a name or a list position (see `ActionModel`).
public enum Request: Equatable, Sendable {
    case status
    case devices
    case pair(ttlSecs: UInt64?)
    case confirm(accept: Bool)
    case unpair(device: String)
    case hideRevokedDevice(fingerprint: String)
    case hideAllRevokedDevices
    case ping(device: String)
    case grant(device: String, capability: String, granted: Bool)
    case send(device: String, path: String)
    case transfers
    case watchFileOffers
    case fileDecision(transfer: String, accept: Bool)
    case cancelTransfer(transfer: String)
    case clipboardStatus
    case clipboardSend(device: String, sensitive: Bool)
    case clipboardApply(device: String)
    case clipboardPolicy(device: String, flag: ClipboardFlag, enabled: Bool)
    case notificationsStatus
    case notificationsPolicy(device: String, setting: NotificationSetting)

    /// The JSON object serde's internally tagged representation produces:
    /// `{"cmd": "<snake_case variant>", <fields>}`.
    public var jsonObject: [String: Any] {
        switch self {
        case .status: return ["cmd": "status"]
        case .devices: return ["cmd": "devices"]
        case let .pair(ttl):
            return ["cmd": "pair", "ttl_secs": ttl.map { NSNumber(value: $0) } ?? NSNull()]
        case let .confirm(accept): return ["cmd": "confirm", "accept": accept]
        case let .unpair(device): return ["cmd": "unpair", "device": device]
        case let .hideRevokedDevice(fingerprint):
            return ["cmd": "hide_revoked_device", "fingerprint": fingerprint]
        case .hideAllRevokedDevices: return ["cmd": "hide_all_revoked_devices"]
        case let .ping(device): return ["cmd": "ping", "device": device]
        case let .grant(device, capability, granted):
            return ["cmd": "grant", "device": device, "capability": capability, "granted": granted]
        case let .send(device, path): return ["cmd": "send", "device": device, "path": path]
        case .transfers: return ["cmd": "transfers"]
        case .watchFileOffers: return ["cmd": "watch_file_offers"]
        case let .fileDecision(transfer, accept):
            return ["cmd": "file_decision", "transfer": transfer, "accept": accept]
        case let .cancelTransfer(transfer): return ["cmd": "cancel_transfer", "transfer": transfer]
        case .clipboardStatus: return ["cmd": "clipboard_status"]
        case let .clipboardSend(device, sensitive):
            return ["cmd": "clipboard_send", "device": device, "sensitive": sensitive]
        case let .clipboardApply(device): return ["cmd": "clipboard_apply", "device": device]
        case let .clipboardPolicy(device, flag, enabled):
            return ["cmd": "clipboard_policy", "device": device, "flag": flag.rawValue, "enabled": enabled]
        case .notificationsStatus: return ["cmd": "notifications_status"]
        case let .notificationsPolicy(device, setting):
            return ["cmd": "notifications_policy", "device": device, "setting": setting.jsonObject]
        }
    }

    /// One line of the wire format: the JSON object and a newline.
    public func line() throws -> Data {
        var data = try JSONSerialization.data(withJSONObject: jsonObject, options: [.sortedKeys])
        data.append(0x0A)
        return data
    }
}

// MARK: - Reports

/// How a known device stands right now (`DeviceState`).
public enum DeviceState: String, Decodable, Equatable, Sendable {
    case revoked
    case connected
    case stale
    case disconnected
    /// A state a newer agent knows and this build does not.
    case unknown

    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = DeviceState(rawValue: raw) ?? .unknown
    }
}

public struct BatteryReport: Decodable, Equatable, Sendable {
    public let percentage: UInt32
    public let chargingState: String
    public let ageSecs: UInt64
    public let stale: Bool
}

public struct ConnectionReport: Decodable, Equatable, Sendable {
    public let deviceId: String
    public let deviceName: String
    public let fingerprintShort: String
    public let negotiatedCapabilities: [String]
    public let battery: BatteryReport?
    public let sessionId: UInt64
    public let state: DeviceState
    public let silentSecs: UInt64
}

public struct DeviceReport: Decodable, Equatable, Sendable, Identifiable {
    public let deviceId: String
    public let deviceName: String
    public let platform: String
    public let fingerprint: String
    public let fingerprintShort: String
    public let pairedAtUnix: Int64
    public let grantedCapabilities: [String]
    public let revoked: Bool
    public let paired: Bool
    public let connected: Bool
    public let state: DeviceState
    public let silentSecs: UInt64?
    public let lastSeenSecsAgo: UInt64?
    public let battery: BatteryReport?

    /// The fingerprint: the one stable identity a device has.
    public var id: String { fingerprint }
}

public struct MigrationReport: Decodable, Equatable, Sendable {
    public let source: String
    public let migratedAtUnix: UInt64
    public let thisRun: Bool
}

public struct StatusReport: Decodable, Equatable, Sendable {
    public let deviceName: String
    public let deviceId: String
    public let fingerprint: String
    public let fingerprintShort: String
    public let keyBacking: String
    public let listenPort: UInt16
    public let listenFamilies: String
    public let protocolVersionMin: UInt32
    public let protocolVersionMax: UInt32
    public let capabilities: [String]
    public let pairedDevices: Int
    public let connections: [ConnectionReport]
    public let devices: [DeviceReport]
    public let pairingActive: Bool
    public let migratedFrom: MigrationReport?
    public let legacyPartialFiles: [String]

    enum CodingKeys: String, CodingKey {
        case deviceName, deviceId, fingerprint, fingerprintShort, keyBacking, listenPort
        case listenFamilies, protocolVersionMin, protocolVersionMax, capabilities
        case pairedDevices, connections, devices, pairingActive, migratedFrom
        case legacyPartialFiles
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        deviceName = try c.decode(String.self, forKey: .deviceName)
        deviceId = try c.decode(String.self, forKey: .deviceId)
        fingerprint = try c.decode(String.self, forKey: .fingerprint)
        fingerprintShort = try c.decode(String.self, forKey: .fingerprintShort)
        // `#[serde(default = "default_key_backing")]`
        keyBacking = try c.decodeIfPresent(String.self, forKey: .keyBacking) ?? "software"
        listenPort = try c.decode(UInt16.self, forKey: .listenPort)
        listenFamilies = try c.decode(String.self, forKey: .listenFamilies)
        protocolVersionMin = try c.decode(UInt32.self, forKey: .protocolVersionMin)
        protocolVersionMax = try c.decode(UInt32.self, forKey: .protocolVersionMax)
        capabilities = try c.decode([String].self, forKey: .capabilities)
        pairedDevices = try c.decode(Int.self, forKey: .pairedDevices)
        connections = try c.decode([ConnectionReport].self, forKey: .connections)
        devices = try c.decode([DeviceReport].self, forKey: .devices)
        pairingActive = try c.decode(Bool.self, forKey: .pairingActive)
        migratedFrom = try c.decodeIfPresent(MigrationReport.self, forKey: .migratedFrom)
        legacyPartialFiles = try c.decodeIfPresent([String].self, forKey: .legacyPartialFiles) ?? []
    }
}

public struct TransferReport: Decodable, Equatable, Sendable, Identifiable {
    public let transferId: String
    public let seq: UInt64
    public let deviceName: String
    public let fingerprintShort: String
    public let direction: String
    public let filename: String
    public let mimeType: String
    public let sizeBytes: UInt64
    public let bytesTransferred: UInt64
    public let percentage: UInt8?
    public let state: String
    public let failure: String?
    public let failureCode: String?
    public let storedAt: String?

    public var id: String { transferId }

    enum CodingKeys: String, CodingKey {
        case transferId, seq, deviceName, fingerprintShort, direction, filename, mimeType
        case sizeBytes, bytesTransferred, percentage, state, failure, failureCode, storedAt
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        transferId = try c.decode(String.self, forKey: .transferId)
        seq = try c.decodeIfPresent(UInt64.self, forKey: .seq) ?? 0
        deviceName = try c.decode(String.self, forKey: .deviceName)
        fingerprintShort = try c.decode(String.self, forKey: .fingerprintShort)
        direction = try c.decode(String.self, forKey: .direction)
        filename = try c.decode(String.self, forKey: .filename)
        mimeType = try c.decode(String.self, forKey: .mimeType)
        sizeBytes = try c.decode(UInt64.self, forKey: .sizeBytes)
        bytesTransferred = try c.decode(UInt64.self, forKey: .bytesTransferred)
        percentage = try c.decodeIfPresent(UInt8.self, forKey: .percentage)
        state = try c.decode(String.self, forKey: .state)
        failure = try c.decodeIfPresent(String.self, forKey: .failure)
        failureCode = try c.decodeIfPresent(String.self, forKey: .failureCode)
        storedAt = try c.decodeIfPresent(String.self, forKey: .storedAt)
    }
}

/// `pliwee_control::transfer_state`.
public enum TransferState {
    public static let offered = "offered"
    public static let waitingAccept = "waiting_accept"
    public static let transferring = "transferring"
    public static let verifying = "verifying"
    public static let completed = "completed"
    public static let failed = "failed"
    public static let cancelled = "cancelled"
    public static let terminal: Set<String> = [completed, failed, cancelled]

    /// An unknown state — a newer agent — is *not* terminal: shown as in
    /// flight, it corrects itself on the next poll, whereas a wrongly
    /// finished one would report an outcome that never happened.
    public static func isTerminal(_ state: String) -> Bool { terminal.contains(state) }
}

/// `pliwee_control::transfer_direction`.
public enum TransferDirection {
    public static let sending = "sending"
    public static let receiving = "receiving"
}

public struct ClipboardPeerReport: Decodable, Equatable, Sendable {
    public let deviceId: String
    public let deviceName: String
    public let fingerprintShort: String
    public let granted: Bool
    public let revoked: Bool
    public let connected: Bool
    public let allowSend: Bool
    public let allowReceive: Bool
    public let autoSend: Bool
    public let autoReceive: Bool
    public let lastOutcome: String?
}

public struct PendingClipReport: Decodable, Equatable, Sendable {
    public let deviceName: String
    public let fingerprintShort: String
    public let bytes: Int
    public let hashPrefix: String
    public let sensitive: Bool
    public let originDeviceId: String
    public let ageSecs: UInt64
}

public struct ClipboardStatusReport: Decodable, Equatable, Sendable {
    public let enabled: Bool
    public let backend: String
    public let backendDetail: String
    public let backendAvailable: Bool
    public let watchAvailable: Bool
    public let sensitiveAvailable: Bool
    public let sensitiveDetail: String
    public let eventCacheEntries: Int
    public let suppressionCacheEntries: Int
    public let peers: [ClipboardPeerReport]
    public let pending: [PendingClipReport]

    enum CodingKeys: String, CodingKey {
        case enabled, backend, backendDetail, backendAvailable, watchAvailable
        case sensitiveAvailable, sensitiveDetail, eventCacheEntries, suppressionCacheEntries
        case peers, pending
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decode(Bool.self, forKey: .enabled)
        backend = try c.decode(String.self, forKey: .backend)
        backendDetail = try c.decode(String.self, forKey: .backendDetail)
        backendAvailable = try c.decodeIfPresent(Bool.self, forKey: .backendAvailable) ?? true
        watchAvailable = try c.decode(Bool.self, forKey: .watchAvailable)
        sensitiveAvailable = try c.decodeIfPresent(Bool.self, forKey: .sensitiveAvailable) ?? true
        sensitiveDetail = try c.decodeIfPresent(String.self, forKey: .sensitiveDetail) ?? ""
        eventCacheEntries = try c.decode(Int.self, forKey: .eventCacheEntries)
        suppressionCacheEntries = try c.decode(Int.self, forKey: .suppressionCacheEntries)
        peers = try c.decode([ClipboardPeerReport].self, forKey: .peers)
        pending = try c.decode([PendingClipReport].self, forKey: .pending)
    }
}

public struct NotificationPeerReport: Decodable, Equatable, Sendable {
    public let deviceId: String
    public let deviceName: String
    public let fingerprintShort: String
    public let granted: Bool
    public let revoked: Bool
    public let connected: Bool
    public let allowMirror: Bool
    public let whenLocked: String
    public let allowDismissSync: Bool
    public let mirrors: Int
    public let displayed: Int
    public let peerIsSource: Bool
}

public struct NotificationsStatusReport: Decodable, Equatable, Sendable {
    public let enabled: Bool
    public let backend: String
    public let backendDetail: String
    public let available: Bool
    public let lockSource: String
    public let lockDetail: String
    public let locked: Bool
    public let mirrors: Int
    public let peers: [NotificationPeerReport]
}

public struct FileOfferRequest: Decodable, Equatable, Sendable, Identifiable {
    /// Full hex. What a `fileDecision` must name, exactly.
    public let transferId: String
    /// The trust store's name for this fingerprint — never the name in the
    /// offer, so a peer cannot rename itself into looking like another.
    public let deviceName: String
    public let deviceId: String
    public let fingerprint: String
    public let fingerprintShort: String
    /// Sanitised by the agent.
    public let filename: String
    public let sizeBytes: UInt64
    /// Advisory and peer-supplied. Shown, never acted on.
    public let mimeType: String

    public var id: String { transferId }
}

// MARK: - Replies

/// A single-shot reply (`Response`), externally tagged: `{"status": {...}}`.
public enum Response: Equatable, Sendable {
    case status(StatusReport)
    case devices([DeviceReport])
    case transfers([TransferReport])
    case clipboard(ClipboardStatusReport)
    case notifications(NotificationsStatusReport)
    case pong(rttMs: UInt64)
    case ok(message: String)
    case error(message: String)
}

private struct Message: Decodable, Equatable, Sendable { let message: String }
private struct Pong: Decodable, Equatable, Sendable { let rttMs: UInt64 }

extension Response: Decodable {
    private enum Tag: String, CodingKey {
        case status, devices, transfers, clipboard, notifications, pong, ok, error
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Tag.self)
        guard let tag = c.allKeys.first, c.allKeys.count == 1 else {
            throw DecodingError.dataCorrupted(.init(
                codingPath: decoder.codingPath,
                debugDescription: "a response has exactly one tag, found \(c.allKeys.map(\.stringValue))"
            ))
        }
        switch tag {
        case .status: self = .status(try c.decode(StatusReport.self, forKey: tag))
        case .devices: self = .devices(try c.decode([DeviceReport].self, forKey: tag))
        case .transfers: self = .transfers(try c.decode([TransferReport].self, forKey: tag))
        case .clipboard: self = .clipboard(try c.decode(ClipboardStatusReport.self, forKey: tag))
        case .notifications:
            self = .notifications(try c.decode(NotificationsStatusReport.self, forKey: tag))
        case .pong: self = .pong(rttMs: try c.decode(Pong.self, forKey: tag).rttMs)
        case .ok: self = .ok(message: try c.decode(Message.self, forKey: tag).message)
        case .error: self = .error(message: try c.decode(Message.self, forKey: tag).message)
        }
    }
}

/// A streamed event (`Event`), internally tagged: `{"event": "...", ...}`.
public enum Event: Equatable, Sendable {
    case pairingReady(payload: String, qrAscii: String, expiresInSecs: UInt64)
    case confirmRequest(deviceName: String, deviceId: String, fingerprint: String, fingerprintShort: String)
    case finished(status: String, detail: String)
    case transferProgress(TransferReport)
    case fileApprovalReady(unattended: Bool)
    case fileOfferRequest(FileOfferRequest)
    case fileOfferWithdrawn(transferId: String, reason: String)
    /// An event a newer agent sends and this build does not know. Ignored,
    /// never guessed at.
    case unknown(String)
}

extension Event: Decodable {
    private enum Keys: String, CodingKey {
        case event, payload, qrAscii, expiresInSecs, deviceName, deviceId, fingerprint
        case fingerprintShort, status, detail, unattended, transferId, reason
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        let tag = try c.decode(String.self, forKey: .event)
        switch tag {
        case "pairing_ready":
            self = .pairingReady(
                payload: try c.decode(String.self, forKey: .payload),
                qrAscii: try c.decode(String.self, forKey: .qrAscii),
                expiresInSecs: try c.decode(UInt64.self, forKey: .expiresInSecs)
            )
        case "confirm_request":
            self = .confirmRequest(
                deviceName: try c.decode(String.self, forKey: .deviceName),
                deviceId: try c.decode(String.self, forKey: .deviceId),
                fingerprint: try c.decode(String.self, forKey: .fingerprint),
                fingerprintShort: try c.decode(String.self, forKey: .fingerprintShort)
            )
        case "finished":
            self = .finished(
                status: try c.decode(String.self, forKey: .status),
                detail: try c.decode(String.self, forKey: .detail)
            )
        case "transfer_progress":
            self = .transferProgress(try TransferReport(from: decoder))
        case "file_approval_ready":
            self = .fileApprovalReady(unattended: try c.decode(Bool.self, forKey: .unattended))
        case "file_offer_request":
            self = .fileOfferRequest(try FileOfferRequest(from: decoder))
        case "file_offer_withdrawn":
            self = .fileOfferWithdrawn(
                transferId: try c.decode(String.self, forKey: .transferId),
                reason: try c.decode(String.self, forKey: .reason)
            )
        default:
            self = .unknown(tag)
        }
    }
}

/// One line of a streaming exchange: the agent sends events, but may answer a
/// stream request with a plain `Response` — `{"error": {...}}` — instead.
public enum StreamMessage: Equatable, Sendable {
    case event(Event)
    case response(Response)
}

// MARK: - Decoding

public enum ControlCoding {
    /// The decoder every reply goes through: serde's `snake_case` keys become
    /// Swift's camelCase properties.
    public static func decoder() -> JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }

    public static func response(from line: Data) throws -> Response {
        try decoder().decode(Response.self, from: line)
    }

    public static func streamMessage(from line: Data) throws -> StreamMessage {
        let object = try JSONSerialization.jsonObject(with: line) as? [String: Any]
        if object?["event"] != nil {
            return .event(try decoder().decode(Event.self, from: line))
        }
        return .response(try decoder().decode(Response.self, from: line))
    }
}
