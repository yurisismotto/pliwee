// Pliwee for macOS.
//
// A menu-bar application with one main window. The menu-bar item is always
// there while the app runs; the window comes and goes; the agent (`pliweed`)
// is a separate per-user service and outlives both. Closing the window never
// quits the app, and quitting the app never stops the service — the same
// split as `pliweed` and `pliwee-gui` on Linux.

import AppKit
import PliweeKit
import SwiftUI

@main
struct PliweeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        MenuBarExtra {
            MenuBarContent(model: delegate.model, windows: delegate)
        } label: {
            MenuBarLabel(health: delegate.model.health)
        }
        .menuBarExtraStyle(.menu)
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") { delegate.showMainWindow(page: .settings) }
                    .keyboardShortcut(",", modifiers: .command)
            }
            CommandGroup(replacing: .newItem) {}
        }
    }
}

/// Opens and focuses the app's windows. The menu-bar content asks for them
/// through this, so the menu never has to know how a window is made.
@MainActor
protocol WindowPresenter: AnyObject {
    func showMainWindow(page: Page?)
    func showPairing()
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, WindowPresenter, NSWindowDelegate {
    let model = AppModel()
    private var mainWindow: NSWindow?
    private var offerPanel: NSPanel?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // A second copy of the app must not become a second menu-bar item or
        // a second approval provider: hand over to the running one and quit,
        // before the model has touched the agent. See PliweeKit/SingleInstance.
        if yieldToRunningInstance() { return }
        // No Dock icon while only the menu-bar item is showing.
        // `LSUIElement` says the same thing in Info.plist; this also covers
        // `swift run`, where there is no Info.plist.
        NSApp.setActivationPolicy(.accessory)
        model.presentOffers = { [weak self] in self?.updateOfferPanel() }
        model.start()
        // Opened at login, the app stays in the menu bar; opened by a person,
        // it shows its window.
        if !Self.launchedAsLoginItem {
            showMainWindow(page: nil)
        }
        #if DEBUG
        DebugHooks.run(delegate: self)
        #endif
    }

    #if DEBUG
    var debugMainWindow: NSWindow? { mainWindow }
    #endif

    /// `true` when another instance was already running and this one is
    /// quitting in its favour.
    private func yieldToRunningInstance() -> Bool {
        guard let id = Bundle.main.bundleIdentifier else { return false }
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: id)
        let instances = running.map {
            SingleInstance.Instance(pid: $0.processIdentifier, launched: $0.launchDate)
        }
        guard let keep = SingleInstance.instanceToYieldTo(
            selfPID: ProcessInfo.processInfo.processIdentifier,
            running: instances
        ), let other = running.first(where: { $0.processIdentifier == keep.pid }) else {
            return false
        }
        // Opening the running copy sends it a reopen event, which shows its
        // window (`applicationShouldHandleReopen`): what the person asked for.
        if let url = other.bundleURL {
            NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
        } else {
            other.activate()
        }
        NSApp.terminate(nil)
        return true
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationWillTerminate(_ notification: Notification) {
        model.shutdown()
    }

    /// Reopening the app from Finder or Spotlight while it runs shows the
    /// window, as any Mac app does.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showMainWindow(page: nil)
        return true
    }

    private static var launchedAsLoginItem: Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent else { return false }
        return event.eventID == AEEventID(kAEOpenApplication)
            && event.paramDescriptor(forKeyword: AEKeyword(keyAEPropData))?.enumCodeValue
            == OSType(keyAELaunchedAsLogInItem)
    }

    // MARK: - The main window

    func showMainWindow(page: Page?) {
        if let page { model.page = page }
        if mainWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 920, height: 620),
                styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            window.title = "Pliwee"
            window.contentMinSize = NSSize(width: 760, height: 480)
            window.isReleasedWhenClosed = false
            window.setFrameAutosaveName("PliweeMainWindow")
            window.contentViewController = NSHostingController(rootView: MainWindow(model: model))
            window.delegate = self
            if !window.setFrameUsingName("PliweeMainWindow") { window.center() }
            mainWindow = window
        }
        // A Dock icon and an app menu while the window is open, so it can be
        // found with ⌘-Tab like any other window.
        NSApp.setActivationPolicy(.regular)
        model.windowVisible = true
        mainWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate()
        Task { await model.refresh() }
    }

    func showPairing() {
        showMainWindow(page: .devices)
        model.beginPairing()
    }

    func windowWillClose(_ notification: Notification) {
        guard (notification.object as? NSWindow) === mainWindow else { return }
        model.windowVisible = false
        model.endPairing()
        // Back to a menu-bar-only app. The process, the menu-bar item and the
        // agent all carry on.
        NSApp.setActivationPolicy(.accessory)
    }

    // MARK: - The incoming-file prompt

    /// Shows the prompt while any offer is waiting and closes it when none
    /// is. A floating panel, so it is seen even when Pliwee is not the
    /// frontmost app — the person at the machine is the last check on what a
    /// paired device writes to it.
    private func updateOfferPanel() {
        if model.offers.isEmpty {
            offerPanel?.orderOut(nil)
            return
        }
        if offerPanel == nil {
            let panel = NSPanel(
                contentRect: NSRect(x: 0, y: 0, width: 420, height: 260),
                styleMask: [.titled, .closable, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            panel.title = "Incoming File"
            panel.level = .floating
            panel.isReleasedWhenClosed = false
            panel.hidesOnDeactivate = false
            panel.contentViewController = NSHostingController(rootView: FileOfferView(model: model))
            panel.delegate = self
            offerPanel = panel
        }
        offerPanel?.center()
        offerPanel?.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    /// Closing the prompt is declining it: every way out that is not the
    /// Accept button is a decline.
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if sender === offerPanel {
            model.declineAll()
        }
        return true
    }
}
