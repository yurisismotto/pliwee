// The menu-bar item.
//
// Shows only what the agent actually reports, and offers only actions that
// have a control request behind them. A disabled item says why in its help
// text, the same sentence the main window would show.

import AppKit
import PliweeKit
import SwiftUI

struct MenuBarLabel: View {
    let health: ServiceHealth

    var body: some View {
        // The brand mark as a template image, so macOS draws it in the menu
        // bar's own colour; dimmed while the service is not running.
        Image(nsImage: BrandAssets.menuBarIcon(active: health.isRunning))
            .accessibilityLabel("Pliwee, \(health.title)")
    }
}

struct MenuBarContent: View {
    let model: AppModel
    let windows: WindowPresenter

    var body: some View {
        Text("Pliwee — \(model.health.title)")
        Text(model.health.headline)

        if model.health.isRunning {
            Divider()
            if model.trusted.isEmpty {
                Text("No paired devices")
            } else {
                Section("Devices") {
                    ForEach(model.trusted) { peer in
                        Text("\(peer.name) — \(peer.subtitle)")
                    }
                }
            }
        }

        Divider()

        Button("Open Pliwee") { windows.showMainWindow(page: nil) }
            .keyboardShortcut("o")

        Button("Pair Device…") { windows.showPairing() }
            .disabled(!model.health.isRunning)

        sendMenu(
            title: "Send File to",
            action: model.sendFileAction,
            perform: { model.chooseAndSendFile(to: $0) }
        )
        sendMenu(
            title: "Send Clipboard to",
            action: model.sendClipboardAction,
            perform: { model.sendClipboard(to: $0) }
        )

        if !model.offers.isEmpty {
            Divider()
            Button("Incoming File (\(model.offers.count))…") { model.presentOffers?() }
        }

        Divider()

        serviceItem

        Button("Settings…") { windows.showMainWindow(page: .settings) }
            .keyboardShortcut(",")

        Divider()

        Button("Quit Pliwee") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }

    /// One submenu per action, one row per trusted device. A row is enabled
    /// only when the action is ready for that device.
    @ViewBuilder
    private func sendMenu(title: String, action: @escaping (PeerCard) -> Action, perform: @escaping (PeerCard) -> Void) -> some View {
        Menu(title) {
            if model.trusted.isEmpty {
                Text("No paired devices")
            }
            ForEach(model.trusted) { peer in
                let availability = action(peer)
                Button(peer.name) { perform(peer) }
                    .disabled(!availability.isReady)
                    .help(availability.reason ?? "")
            }
        }
        .disabled(!model.health.isRunning)
    }

    /// Starting the service is offered where it is off; nothing is offered
    /// that ServiceManagement cannot do in this build.
    @ViewBuilder
    private var serviceItem: some View {
        switch model.health {
        case .off:
            Button("Turn On Pliwee Service") { model.setAgentEnabled(true) }
                .disabled(model.registration == .notFound)
        case .needsApproval:
            Button("Open Login Items Settings…") { model.agent.openLoginItemsSettings() }
        default:
            EmptyView()
        }
    }
}
