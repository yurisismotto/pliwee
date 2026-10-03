// Settings: the background service, opening at login, and where things are.

import AppKit
import PliweeKit
import SwiftUI

struct SettingsView: View {
    let model: AppModel

    var body: some View {
        PageScroll {
            Card("Background service") {
                SwitchRow(
                    title: "Run Pliwee in the background",
                    caption: "Keeps Pliwee connected to your devices while this window is closed, and starts it when you log in. Nothing listens on the network until this is on.",
                    isOn: Binding(
                        get: { model.registration == .enabled || model.registration == .requiresApproval },
                        set: { model.setAgentEnabled($0) }
                    )
                )
                .disabled(model.registration == .notFound)

                Label(model.health.headline, systemImage: model.health.symbol)
                    .foregroundStyle(model.health.color)
                    .font(.callout)

                switch model.registration {
                case .enabled where !model.health.isRunning && model.health != .starting:
                    Button("Restart Service") { model.restartAgent() }
                        .help("Turns the background service off and on again.")
                case .requiresApproval:
                    Button("Open Login Items Settings…") { model.agent.openLoginItemsSettings() }
                case .notFound:
                    Text("This copy of Pliwee is not running from Pliwee.app, so it cannot manage the background service. Start pliweed yourself, or build the app with macos/scripts/build-app.sh.")
                        .font(.caption).foregroundStyle(.secondary)
                default:
                    EmptyView()
                }
            }

            Card("Menu bar") {
                SwitchRow(
                    title: "Open Pliwee at login",
                    caption: "Shows the Pliwee menu-bar item when you log in. The background service runs either way.",
                    isOn: Binding(
                        get: { model.openAtLogin == .enabled },
                        set: { model.setOpenAtLogin($0) }
                    )
                )
                .disabled(model.openAtLogin == .notFound)
                if model.openAtLogin == .requiresApproval {
                    Button("Open Login Items Settings…") { model.agent.openLoginItemsSettings() }
                }
            }

            if let notifications = model.notifications {
                Card("Notifications") {
                    FactRow(label: "Showing notifications", value: notifications.available ? "Available" : "Not available on this Mac")
                    FactRow(label: "Detail", value: notifications.backendDetail)
                    Text("Mirroring your phone's notifications needs a notification service on this Mac. Pliwee for macOS does not provide one yet, so it tells your devices not to send any.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }

            Card("Locations") {
                location("Identity and pairings", model.paths.dataDirectory)
                location("Received files", model.paths.downloadsDirectory)
                location("Service log", model.paths.logFile)
                FactRow(label: "Control socket", value: model.paths.controlSocket.path, monospaced: true)
                Text("The private key of this Mac's identity is kept in your login keychain, not in a file.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Card("About") {
                FactRow(label: "Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "development build")
                if let status = model.status {
                    FactRow(label: "Service protocol", value: "v\(status.protocolVersionMin)–v\(status.protocolVersionMax)")
                }
            }
        }
    }

    private func location(_ label: String, _ url: URL) -> some View {
        HStack {
            FactRow(label: label, value: url.path.replacingOccurrences(of: model.paths.home.path, with: "~"))
            Button("Show") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
                .disabled(!FileManager.default.fileExists(atPath: url.path))
        }
    }
}
