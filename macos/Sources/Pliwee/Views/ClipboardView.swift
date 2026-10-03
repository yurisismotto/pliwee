// Clipboard: what this Mac's clipboard can do, and per-device policy.
//
// The agent never sends clipboard content over the control socket, so
// nothing here can show any: a pending clip is a size, a hash prefix and an
// age, enough to tell two apart and to decide whether to apply one.

import PliweeKit
import SwiftUI

struct ClipboardView: View {
    let model: AppModel

    var body: some View {
        if model.status == nil {
            ServiceUnavailableView(model: model)
        } else if let report = model.clipboard {
            PageScroll {
                Card("This Mac") {
                    FactRow(label: "Clipboard", value: report.backendAvailable ? "Available" : "Unavailable")
                    FactRow(label: "Backend", value: report.backendDetail)
                    FactRow(
                        label: "Sensitive clips",
                        value: report.sensitiveAvailable
                            ? "Marked as concealed, so clipboard managers skip them"
                            : (report.sensitiveDetail.isEmpty ? "Not supported" : report.sensitiveDetail)
                    )
                    FactRow(
                        label: "Automatic sending",
                        value: report.watchAvailable
                            ? "Available"
                            : "Not available on macOS — send the clipboard by hand"
                    )
                }

                if !report.pending.isEmpty {
                    Card("Waiting to be applied") {
                        Text("These clips arrived while automatic receiving was off. Apply one to put it on this Mac's clipboard.")
                            .font(.callout).foregroundStyle(.secondary)
                        ForEach(report.pending, id: \.hashPrefix) { clip in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text("From \(clip.deviceName)\(clip.sensitive ? " · sensitive" : "")")
                                    Text("\(clip.bytes) bytes · \(clip.hashPrefix) · \(Format.ago(seconds: clip.ageSecs))")
                                        .font(.caption.monospaced()).foregroundStyle(.secondary)
                                }
                                Spacer()
                                if let peer = model.trusted.first(where: { $0.fingerprintShort == clip.fingerprintShort }) {
                                    Button("Apply") { model.applyPendingClip(from: peer) }
                                }
                            }
                        }
                    }
                }

                ForEach(model.trusted) { peer in
                    ClipboardPeerCard(model: model, peer: peer, report: report)
                }
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct ClipboardPeerCard: View {
    let model: AppModel
    let peer: PeerCard
    let report: ClipboardStatusReport

    var body: some View {
        Card(peer.name) {
            if let policy = Actions.clipboardPolicy(in: report, for: peer) {
                if !policy.granted {
                    Text("Clipboard is not allowed for this device. Allow it on the Devices page.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                flag(.send, "Send this Mac's clipboard to it", on: policy.allowSend, policy: policy)
                flag(.receive, "Accept its clipboard", on: policy.allowReceive, policy: policy)
                flag(.autoSend, "Send automatically when I copy", on: policy.autoSend, policy: policy)
                    .disabled(!report.watchAvailable)
                    .help(report.watchAvailable ? "" : "macOS has no clipboard-change notification, and Pliwee does not poll the clipboard.")
                flag(.autoReceive, "Apply its clipboard automatically", on: policy.autoReceive, policy: policy)
                if let outcome = policy.lastOutcome {
                    FactRow(label: "Last result", value: outcome)
                }
                let action = model.sendClipboardAction(peer)
                Button("Send Clipboard Now") { model.sendClipboard(to: peer) }
                    .disabled(!action.isReady)
                    .help(action.reason ?? "")
            } else {
                Text("The Pliwee service has no clipboard policy for this device.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func flag(_ flag: ClipboardFlag, _ title: String, on: Bool, policy: ClipboardPeerReport) -> some View {
        SwitchRow(title: title, isOn: Binding(
            get: { on },
            set: { model.setClipboardFlag(flag, enabled: $0, for: peer) }
        ))
        .disabled(!policy.granted)
    }
}
