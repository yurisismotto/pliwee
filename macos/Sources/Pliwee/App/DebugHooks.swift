// Validation hooks for debug builds. Compiled out of release builds.
//
// The lifecycle of a menu-bar app — open the window, close it, register the
// agent, quit — is what most needs checking and is the hardest to check from
// outside: a screenshot needs Screen Recording permission and a click needs
// Accessibility permission. So a debug build can be told to do those things
// itself and write down what happened:
//
//   PLIWEE_DEBUG_DIR=/tmp/pliwee-check \
//   PLIWEE_DEBUG_STEPS="wait:4,snap:overview,snap:devices,agent:status,close,report,quit" \
//   open build/Pliwee.app            # a bundle built with --debug
//
// Steps run in order, after launch:
//   wait:N        sleep N seconds (the poll keeps running)
//   snap:PAGE     show PAGE in the main window and save it as PAGE.png —
//                 the window's own pixels, which needs no permission
//   open          open/focus the main window, as "Open Pliwee" does
//   close         close the main window, as its close button does
//   agent:ACTION  register | unregister | status — the real SMAppService call
//   report        write the menu's data and the app's state to steps.log
//   pair          begin pairing, as "Pair Device…" does; writes the QR
//                 payload to pair-payload.txt once the agent sends it
//   confirm:yes|no  wait for the fingerprint question and answer it
//   snapsheet:N   save the sheet in front of the main window as N.png
//   grant:CAP     allow CAP for the first paired device, as its switch does
//   offer:yes|no  wait for an incoming-file prompt, save it as offer.png,
//                 and press Accept or Decline
//   quit          NSApp.terminate, as "Quit Pliwee" does
//
// Nothing here is reachable in a release build, and nothing here changes
// what the app does; it calls the same methods the menu and buttons call.

#if DEBUG
import AppKit
import PliweeKit
import SwiftUI

@MainActor
enum DebugHooks {
    static func run(delegate: AppDelegate) {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["PLIWEE_DEBUG_DIR"], let steps = env["PLIWEE_DEBUG_STEPS"] else { return }
        let out = URL(fileURLWithPath: dir, isDirectory: true)
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let log = out.appendingPathComponent("steps.log")
        try? Data().write(to: log)
        func write(_ line: String) {
            let stamped = "\(ISO8601DateFormatter().string(from: Date())) \(line)\n"
            if let handle = try? FileHandle(forWritingTo: log) {
                handle.seekToEndOfFile()
                handle.write(Data(stamped.utf8))
                try? handle.close()
            }
        }
        Task { @MainActor in
            for step in steps.split(separator: ",").map(String.init) {
                let parts = step.split(separator: ":", maxSplits: 1).map(String.init)
                let name = parts[0]
                let arg = parts.count > 1 ? parts[1] : ""
                switch name {
                case "wait":
                    try? await Task.sleep(nanoseconds: UInt64((Double(arg) ?? 1) * 1_000_000_000))
                case "open":
                    delegate.showMainWindow(page: nil)
                    write("open: window visible=\(delegate.debugMainWindow?.isVisible ?? false) policy=\(policy())")
                case "close":
                    delegate.debugMainWindow?.performClose(nil)
                    try? await Task.sleep(nanoseconds: 500_000_000)
                    write("close: window visible=\(delegate.debugMainWindow?.isVisible ?? false) policy=\(policy()) app running=true")
                case "snap":
                    guard let page = Page(rawValue: arg) else { write("snap: unknown page \(arg)"); continue }
                    delegate.showMainWindow(page: page)
                    try? await Task.sleep(nanoseconds: 1_500_000_000)
                    let file = out.appendingPathComponent("\(arg).png")
                    write("snap \(arg): \(snapshot(delegate.debugMainWindow, to: file) ? "saved" : "FAILED")")
                case "agent":
                    write("agent \(arg): \(agent(arg, model: delegate.model))")
                case "report":
                    await delegate.model.refresh()
                    write(report(delegate))
                case "pair":
                    delegate.showPairing()
                    var payload: String?
                    for _ in 0..<40 {
                        if case let .showingCode(p, _)? = delegate.model.pairing?.phase { payload = p; break }
                        try? await Task.sleep(nanoseconds: 250_000_000)
                    }
                    if let payload {
                        try? Data(payload.utf8).write(to: out.appendingPathComponent("pair-payload.txt"))
                        write("pair: code shown")
                    } else {
                        write("pair: no code: \(String(describing: delegate.model.pairing?.phase))")
                    }
                case "confirm":
                    var asked = false
                    for _ in 0..<240 {
                        if case let .confirming(name, _, short)? = delegate.model.pairing?.phase {
                            write("confirm: asked about \(name) \(short)")
                            asked = true
                            break
                        }
                        try? await Task.sleep(nanoseconds: 250_000_000)
                    }
                    if asked {
                        try? await Task.sleep(nanoseconds: 800_000_000)
                        if let window = delegate.debugMainWindow?.attachedSheet {
                            write("snap confirm-sheet: \(snapshot(window, to: out.appendingPathComponent("pairing-confirm.png")) ? "saved" : "FAILED")")
                        }
                        delegate.model.pairing?.answer(accept: arg == "yes")
                        for _ in 0..<40 {
                            if case .finished? = delegate.model.pairing?.phase { break }
                            try? await Task.sleep(nanoseconds: 250_000_000)
                        }
                    }
                    write("confirm \(arg): \(String(describing: delegate.model.pairing?.phase))")
                case "snapsheet":
                    let window = delegate.debugMainWindow?.attachedSheet
                    write("snapsheet \(arg): \(snapshot(window, to: out.appendingPathComponent("\(arg).png")) ? "saved" : "no sheet")")
                case "endpair":
                    delegate.model.endPairing()
                    write("endpair")
                case "grant":
                    await delegate.model.refresh()
                    if let peer = delegate.model.trusted.first {
                        delegate.model.setGrant(arg, granted: true, for: peer)
                        try? await Task.sleep(nanoseconds: 1_000_000_000)
                        await delegate.model.refresh()
                        let now = delegate.model.trusted.first?.capability(arg)
                        write("grant \(arg) to \(peer.name): granted=\(now?.granted ?? false) live=\(now?.live ?? false)")
                    } else {
                        write("grant \(arg): no paired device")
                    }
                case "offer":
                    var offer: FileOfferRequest?
                    for _ in 0..<240 {
                        offer = delegate.model.offers.first
                        if offer != nil { break }
                        try? await Task.sleep(nanoseconds: 250_000_000)
                    }
                    guard let offer else { write("offer: none arrived"); continue }
                    write("offer: \(offer.filename) \(offer.sizeBytes) bytes from \(offer.deviceName) \(offer.fingerprintShort)")
                    try? await Task.sleep(nanoseconds: 800_000_000)
                    let panel = NSApp.windows.first { $0.title == "Incoming File" && $0.isVisible }
                    write("snap offer: \(snapshot(panel, to: out.appendingPathComponent("offer.png")) ? "saved" : "no panel")")
                    delegate.model.decide(offer, accept: arg == "yes")
                    write("offer \(arg): answered; panel visible=\(panel?.isVisible ?? false)")
                case "quit":
                    write("quit: terminating")
                    NSApp.terminate(nil)
                default:
                    write("unknown step \(step)")
                }
            }
        }
    }

    private static func policy() -> String {
        switch NSApp.activationPolicy() {
        case .regular: return "regular"
        case .accessory: return "accessory"
        case .prohibited: return "prohibited"
        @unknown default: return "unknown"
        }
    }

    private static func snapshot(_ window: NSWindow?, to url: URL) -> Bool {
        guard let view = window?.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { return false }
        return (try? png.write(to: url)) != nil
    }

    private static func agent(_ action: String, model: AppModel) -> String {
        switch action {
        case "register": model.setAgentEnabled(true)
        case "unregister": model.setAgentEnabled(false)
        default: break
        }
        return "registration=\(model.agent.registration) openAtLogin=\(model.agent.openAtLogin) notice=\(model.notice?.text ?? "-")"
    }

    /// What the menu would show, built from the same model calls it makes.
    private static func report(_ delegate: AppDelegate) -> String {
        let model = delegate.model
        var lines = ["report:"]
        lines.append("  menu status: Pliwee — \(model.health.title)")
        lines.append("  menu headline: \(model.health.headline)")
        lines.append("  devices: \(model.trusted.map { "\($0.name) — \($0.subtitle)" })")
        for peer in model.trusted {
            lines.append("  send file to \(peer.name): \(model.sendFileAction(peer))")
            lines.append("  send clipboard to \(peer.name): \(model.sendClipboardAction(peer))")
        }
        lines.append("  registration: \(model.registration), openAtLogin: \(model.openAtLogin)")
        lines.append("  status: \(model.status.map { "\($0.deviceName) \($0.fingerprintShort) port \($0.listenPort) backing \($0.keyBacking) caps \($0.capabilities)" } ?? "none")")
        lines.append("  clipboard: \(model.clipboard.map { "backend=\($0.backend) available=\($0.backendAvailable) watch=\($0.watchAvailable) sensitive=\($0.sensitiveAvailable)" } ?? "none")")
        lines.append("  notifications: \(model.notifications.map { "backend=\($0.backend) available=\($0.available)" } ?? "none")")
        lines.append("  transfers: \(model.transfers.count), offers waiting: \(model.offers.count)")
        lines.append("  window visible: \(delegate.debugMainWindow?.isVisible ?? false), policy: \(policy())")
        let statusItems = (NSApp.windows.filter { String(describing: type(of: $0)).contains("StatusBar") }).count
        lines.append("  status-bar windows owned by this process: \(statusItems)")
        return lines.joined(separator: "\n")
    }
}
#endif
