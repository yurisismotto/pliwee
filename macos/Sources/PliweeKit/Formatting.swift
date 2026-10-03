// Small presentation helpers with no UI framework in them.

import Foundation

public enum Format {
    /// A size in bytes, the way Finder writes it.
    public static func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: count), countStyle: .file)
    }

    /// A full fingerprint, upper-case, in groups of four — the form a person
    /// compares against another screen.
    public static func fingerprint(_ hex: String) -> String {
        let upper = hex.uppercased()
        var groups: [String] = []
        var index = upper.startIndex
        while index < upper.endIndex {
            let end = upper.index(index, offsetBy: 4, limitedBy: upper.endIndex) ?? upper.endIndex
            groups.append(String(upper[index..<end]))
            index = end
        }
        return groups.joined(separator: " ")
    }

    /// "12 seconds ago", "3 minutes ago" — for `last_seen_secs_ago`.
    public static func ago(seconds: UInt64) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter.localizedString(fromTimeInterval: -TimeInterval(seconds))
    }

    /// A transfer's state as a person reads it.
    public static func transferState(_ report: TransferReport) -> String {
        switch report.state {
        case TransferState.offered: return "Offered"
        case TransferState.waitingAccept: return "Waiting for approval"
        case TransferState.transferring:
            if let pct = report.percentage { return "Transferring · \(pct)%" }
            return "Transferring"
        case TransferState.verifying: return "Verifying"
        case TransferState.completed: return "Completed"
        case TransferState.failed: return "Failed"
        case TransferState.cancelled: return "Cancelled"
        default: return report.state
        }
    }

    /// How a pairing session ended, from its `finished` event.
    public static func pairingOutcome(status: String, detail: String) -> String {
        switch status {
        case "paired": return "Paired. Fingerprint \(detail)."
        case "declined": return "Declined. Nothing was paired."
        case "expired": return "The pairing code expired before a device used it."
        case "cancelled": return "Pairing was cancelled."
        default: return detail.isEmpty ? status : "\(status): \(detail)"
        }
    }
}
