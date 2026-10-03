// Files: transfers in this agent run, sending, and where received files go.

import AppKit
import PliweeKit
import SwiftUI

struct FilesView: View {
    let model: AppModel

    var body: some View {
        if model.status == nil {
            ServiceUnavailableView(model: model)
        } else {
            PageScroll {
                Card("Send a file") {
                    let ready = model.trusted.filter { model.sendFileAction($0).isReady }
                    if model.trusted.isEmpty {
                        Text("Pair a device first.").foregroundStyle(.secondary)
                    } else if ready.isEmpty {
                        Text(model.trusted.compactMap { model.sendFileAction($0).reason }.first ?? "")
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        ForEach(ready) { peer in
                            Button("Send to \(peer.name)…") { model.chooseAndSendFile(to: peer) }
                        }
                    }
                }

                Card("Receiving") {
                    Text("Received files are saved in \(model.paths.downloadsDirectory.path.replacingOccurrences(of: model.paths.home.path, with: "~")). An existing file is never overwritten, and every file asks before it is saved.")
                        .foregroundStyle(.secondary)
                    Button("Show in Finder") {
                        NSWorkspace.shared.open(model.paths.downloadsDirectory)
                    }
                    .disabled(!FileManager.default.fileExists(atPath: model.paths.downloadsDirectory.path))
                }

                Card("Transfers") {
                    if model.transfers.isEmpty {
                        Text("No transfers since the Pliwee service started.").foregroundStyle(.secondary)
                    }
                    ForEach(model.sortedTransfers) { transfer in
                        TransferRow(model: model, transfer: transfer)
                        if transfer.id != model.sortedTransfers.last?.id { Divider() }
                    }
                }
            }
        }
    }
}

private struct TransferRow: View {
    let model: AppModel
    let transfer: TransferReport

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: transfer.direction == TransferDirection.receiving ? "arrow.down.circle" : "arrow.up.circle")
                .font(.title2)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text(transfer.filename).font(.body.weight(.medium)).lineLimit(1).truncationMode(.middle)
                Text("\(transfer.direction == TransferDirection.receiving ? "From" : "To") \(transfer.deviceName) · \(Format.bytes(transfer.sizeBytes))")
                    .font(.caption).foregroundStyle(.secondary)
                if !TransferState.isTerminal(transfer.state), let pct = transfer.percentage {
                    ProgressView(value: Double(pct), total: 100).frame(maxWidth: 240)
                }
                Text(transfer.failure ?? Format.transferState(transfer))
                    .font(.caption)
                    .foregroundStyle(stateColor)
            }
            Spacer()
            if TransferState.isTerminal(transfer.state) {
                if transfer.storedAt != nil {
                    Button("Show in Finder") { model.revealInFinder(transfer) }
                }
            } else {
                Button("Cancel") { model.cancel(transfer) }
            }
        }
    }

    private var stateColor: Color {
        switch transfer.state {
        case TransferState.completed: return .token(DesignTokens.Status.success)
        case TransferState.failed: return .token(DesignTokens.Status.error)
        case TransferState.cancelled: return .secondary
        default: return .token(DesignTokens.Status.transferring)
        }
    }
}
