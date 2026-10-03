// Whether a quick action can run, and against which device.
//
// Ported from the GTK Quick Panel (`desktop/gui/src/panel/model.rs`), with
// its one invariant: **a destination is a fingerprint** — never a display
// name, a list index or an address. An action is either `ready` with the
// full fingerprint the agent will be given, or `blocked` with the sentence
// that says why. The view has nothing else to go on.

import Foundation

public enum Action: Equatable, Sendable {
    /// Full fingerprint hex, handed to the agent verbatim; the name is for
    /// the caption only, never routing.
    case ready(fingerprint: String, peerName: String)
    /// Disabled, with the reason said out loud.
    case blocked(reason: String)

    public var isReady: Bool {
        if case .ready = self { return true }
        return false
    }

    public var reason: String? {
        if case let .blocked(reason) = self { return reason }
        return nil
    }
}

public enum Actions {
    private static func preconditions(health: ServiceHealth, peer: PeerCard) -> Action? {
        guard health.isRunning else {
            return .blocked(reason: health.headline)
        }
        if peer.revoked {
            return .blocked(reason: "\(peer.name) is no longer paired.")
        }
        if !peer.link.isLive {
            return .blocked(reason: "\(peer.name) is \(peer.link.label.lowercased()).")
        }
        return nil
    }

    /// Sending a file to `peer`.
    public static func sendFile(health: ServiceHealth, peer: PeerCard) -> Action {
        if let blocked = preconditions(health: health, peer: peer) { return blocked }
        if !peer.files.granted {
            return .blocked(reason: "Files are not enabled for \(peer.name).")
        }
        if !peer.files.live {
            return .blocked(reason: "This session with \(peer.name) has not negotiated file transfer.")
        }
        return .ready(fingerprint: peer.fingerprint, peerName: peer.name)
    }

    /// Sending this Mac's clipboard to `peer`.
    public static func sendClipboard(health: ServiceHealth, peer: PeerCard, report: ClipboardStatusReport?) -> Action {
        if let blocked = preconditions(health: health, peer: peer) { return blocked }
        guard let report else {
            return .blocked(reason: "Waiting for the Pliwee service…")
        }
        if !report.enabled {
            return .blocked(reason: "Clipboard sharing is not enabled on this Mac.")
        }
        if !report.backendAvailable {
            return .blocked(reason: "This session has no working clipboard.")
        }
        if !peer.clipboard.granted {
            return .blocked(reason: "Clipboard is not enabled for \(peer.name).")
        }
        if !peer.clipboard.live {
            return .blocked(reason: "This session with \(peer.name) has not negotiated the clipboard.")
        }
        switch clipboardPolicy(in: report, for: peer) {
        case let policy? where policy.allowSend:
            return .ready(fingerprint: peer.fingerprint, peerName: peer.name)
        case .some:
            return .blocked(reason: "Sending the clipboard to \(peer.name) is turned off.")
        case nil:
            return .blocked(reason: "The Pliwee service has no clipboard policy for \(peer.name).")
        }
    }

    /// The clipboard policy row for `peer`: matched on device id *and* short
    /// fingerprint, and only when exactly one row matches. A policy row is
    /// not a destination, but acting on the wrong device's row would make a
    /// button say "ready" for a device that is turned off.
    public static func clipboardPolicy(in report: ClipboardStatusReport, for peer: PeerCard) -> ClipboardPeerReport? {
        let hits = report.peers.filter {
            $0.deviceId == peer.deviceId && $0.fingerprintShort == peer.fingerprintShort
        }
        return hits.count == 1 ? hits.first : nil
    }
}
