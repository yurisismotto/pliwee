// Overview: this Mac's identity, the service, and the devices at a glance.

import PliweeKit
import SwiftUI

struct OverviewView: View {
    let model: AppModel

    var body: some View {
        if let status = model.status {
            PageScroll {
                header(status)

                Card("Devices") {
                    if model.trusted.isEmpty {
                        Text("No device is paired yet. Pair your phone to start sharing files and your clipboard.")
                            .foregroundStyle(.secondary)
                        Button("Pair Device…") { model.beginPairing() }
                    } else {
                        ForEach(model.trusted) { peer in
                            DeviceRow(peer: peer)
                        }
                    }
                }

                Card("This Mac") {
                    FactRow(label: "Name", value: status.deviceName)
                    FactRow(label: "Fingerprint", value: Format.fingerprint(status.fingerprint), monospaced: true)
                    FactRow(label: "Key storage", value: keyStorage(status.keyBacking))
                    FactRow(label: "Listening on", value: "Port \(status.listenPort) · \(status.listenFamilies)")
                    FactRow(label: "Protocol", value: "v\(status.protocolVersionMin)–v\(status.protocolVersionMax)")
                    FactRow(label: "Capabilities", value: status.capabilities.map(Capability.title(of:)).joined(separator: ", "))
                }
            }
        } else {
            ServiceUnavailableView(model: model)
        }
    }

    private func header(_ status: StatusReport) -> some View {
        HStack(spacing: 16) {
            if let mark = BrandAssets.mark {
                Image(nsImage: mark).resizable().scaledToFit().frame(width: 52, height: 52)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(status.deviceName).font(.title2.weight(.semibold))
                Label(model.health.headline, systemImage: model.health.symbol)
                    .foregroundStyle(model.health.color)
            }
            Spacer()
            Button("Pair Device…") { model.beginPairing() }
                .buttonStyle(.borderedProminent)
        }
    }

    /// The agent reports the backing; this is what it means for the owner.
    private func keyStorage(_ backing: String) -> String {
        switch backing {
        case "software": return "Software key (private key in the login keychain)"
        case "secure-enclave": return "Secure Enclave"
        default: return backing
        }
    }
}

struct DeviceRow: View {
    let peer: PeerCard

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: peer.platform == "android" ? "iphone" : "desktopcomputer")
                .frame(width: 22)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(peer.name).font(.body.weight(.medium))
                HStack(spacing: 4) {
                    Image(systemName: peer.link.symbol).font(.caption2).foregroundStyle(peer.link.color)
                    Text(peer.subtitle).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
        }
        .accessibilityElement(children: .combine)
    }
}
