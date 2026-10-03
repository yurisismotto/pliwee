// Devices: what each paired device may do, and the actions on it.
//
// Pairing establishes who a device is; a grant decides what it may do, and
// nothing is granted by pairing alone (ADR-0008). Every switch here is one
// `grant` request, re-checked by the agent when the device next asks.

import PliweeKit
import SwiftUI

struct DevicesView: View {
    @Bindable var model: AppModel

    var body: some View {
        if model.status == nil {
            ServiceUnavailableView(model: model)
        } else {
            PageScroll {
                HStack {
                    Text("Pair a device by scanning a code with the Pliwee app on your phone. A new device can do nothing until you allow it below.")
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Pair Device…") { model.beginPairing() }
                        .buttonStyle(.borderedProminent)
                }

                if model.trusted.isEmpty {
                    Card { Text("No device is paired yet.").foregroundStyle(.secondary) }
                }
                ForEach(model.trusted) { peer in
                    DeviceCard(model: model, peer: peer)
                }

                if model.revokedCount > 0 {
                    Card("No longer paired") {
                        Text("These devices were unpaired. They cannot connect unless they pair again. Removing one from the list keeps it blocked.")
                            .font(.callout).foregroundStyle(.secondary)
                        ForEach(model.cards.filter(\.revoked)) { peer in
                            HStack {
                                Text(peer.fingerprintShort).font(.body.monospaced())
                                Spacer()
                                Button("Remove from List") { model.removeFromList(peer) }
                            }
                        }
                        if model.revokedCount > 1 {
                            Button("Remove All from List") { model.removeAllRevoked() }
                        }
                    }
                }
            }
            .confirmationDialog(
                "Unpair \(model.unpairCandidate?.name ?? "this device")?",
                isPresented: Binding(get: { model.unpairCandidate != nil }, set: { if !$0 { model.unpairCandidate = nil } }),
                presenting: model.unpairCandidate
            ) { peer in
                Button("Unpair", role: .destructive) { model.unpair(peer) }
            } message: { _ in
                Text("The device will no longer be able to connect to this Mac. To use it again, pair it again.")
            }
        }
    }
}

private struct DeviceCard: View {
    let model: AppModel
    let peer: PeerCard

    var body: some View {
        Card {
            HStack(alignment: .top) {
                DeviceRow(peer: peer)
                Spacer()
                Menu {
                    Button("Check Connection") { model.ping(peer) }
                        .disabled(!peer.link.isLive)
                    Divider()
                    Button("Unpair…", role: .destructive) { model.unpairCandidate = peer }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }

            FactRow(label: "Fingerprint", value: peer.fingerprintShort, monospaced: true)
            if let platform = peer.platformLabel {
                FactRow(label: "Platform", value: platform)
            }
            if !peer.link.isLive, let ago = peer.lastSeenSecsAgo {
                FactRow(label: "Last connected", value: Format.ago(seconds: ago))
            }

            Divider()
            Text("Allowed on this Mac").font(.subheadline.weight(.semibold))
            ForEach(Capability.known, id: \.id) { capability in
                if model.status?.capabilities.contains(capability.id) == true {
                    SwitchRow(
                        title: capability.title,
                        caption: grantDetail(capability.id),
                        isOn: Binding(
                            get: { peer.capability(capability.id).granted },
                            set: { model.setGrant(capability.id, granted: $0, for: peer) }
                        )
                    )
                }
            }

            Divider()
            HStack {
                actionButton("Send File…", systemImage: "arrow.up.doc", action: model.sendFileAction(peer)) {
                    model.chooseAndSendFile(to: peer)
                }
                actionButton("Send Clipboard", systemImage: "doc.on.clipboard", action: model.sendClipboardAction(peer)) {
                    model.sendClipboard(to: peer)
                }
            }
        }
    }

    private func grantDetail(_ capability: String) -> String {
        switch capability {
        case Capability.files: return "Send files to this Mac. Each file still asks before it is saved."
        case Capability.clipboard: return "Exchange clipboard text with this Mac."
        case Capability.battery: return "Share battery levels."
        case Capability.notifications: return "Show this device's notifications. Not available on macOS yet."
        default: return ""
        }
    }

    /// A button that is enabled only when the action is ready, and says why
    /// when it is not.
    private func actionButton(_ title: String, systemImage: String, action: Action, perform: @escaping () -> Void) -> some View {
        Button(action: perform) { Label(title, systemImage: systemImage) }
            .disabled(!action.isReady)
            .help(action.reason ?? "")
    }
}
