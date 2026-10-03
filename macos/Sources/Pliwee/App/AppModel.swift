// The application's state, and every action it can take.
//
// A client of the agent and nothing more, like the GTK application: it
// polls metadata the agent already holds in memory, keeps the two streams the
// protocol has (pairing, and the incoming-file approval prompt), and turns
// each button into one control request the CLI could make too. It adds no
// protocol, no capability and no privilege.

import AppKit
import Foundation
import Observation
import PliweeKit

enum Page: String, CaseIterable, Identifiable {
    case overview, devices, files, clipboard, settings
    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: return "Overview"
        case .devices: return "Devices"
        case .files: return "Files"
        case .clipboard: return "Clipboard"
        case .settings: return "Settings"
        }
    }

    var symbol: String {
        switch self {
        case .overview: return "house"
        case .devices: return "laptopcomputer.and.iphone"
        case .files: return "doc.on.doc"
        case .clipboard: return "list.clipboard"
        case .settings: return "gearshape"
        }
    }
}

/// A one-line outcome shown at the bottom of the window.
struct Notice: Equatable, Identifiable {
    let id = UUID()
    let text: String
    let isError: Bool
}

@MainActor
@Observable
final class AppModel {
    let paths: RuntimePaths
    let client: ControlClient
    let agent: AgentService

    // What the agent last said.
    private(set) var status: StatusReport?
    private(set) var transfers: [TransferReport] = []
    private(set) var clipboard: ClipboardStatusReport?
    private(set) var notifications: NotificationsStatusReport?
    private(set) var health: ServiceHealth = .starting
    private(set) var registration: AgentRegistration = .notFound
    private(set) var openAtLogin: AgentRegistration = .notFound

    // The incoming-file approval prompt.
    private(set) var offers: [FileOfferRequest] = []
    private(set) var approvalUnattended = false

    // Navigation and transient UI state.
    var page: Page = .overview
    var pairing: PairingSession?
    /// The device an "Unpair?" confirmation is open for. Held here rather
    /// than in view state: SwiftUI's `@State` is a compiler macro whose
    /// plugin ships with Xcode only, and this app builds with the Command
    /// Line Tools alone.
    var unpairCandidate: PeerCard?
    private(set) var notice: Notice?
    var windowVisible = false

    /// When this run turned the agent on, for the start-up grace.
    private var enabledAt: Date?
    private var pollTask: Task<Void, Never>?
    private var offersTask: Task<Void, Never>?
    private var offerStream: ControlStream?

    /// Called whenever an offer arrives, to put the prompt in front of the
    /// person. Set by the app delegate.
    var presentOffers: (() -> Void)?

    init(paths: RuntimePaths = .forCurrentUser()) {
        self.paths = paths
        self.client = ControlClient(paths: paths)
        self.agent = AgentService()
    }

    // MARK: - Derived

    var cards: [PeerCard] { status.map(DeviceDirectory.cards(from:)) ?? [] }
    var trusted: [PeerCard] { cards.filter { !$0.revoked } }
    var revokedCount: Int { cards.filter(\.revoked).count }

    var sortedTransfers: [TransferReport] { transfers.sorted { $0.seq > $1.seq } }

    func sendFileAction(_ peer: PeerCard) -> Action { Actions.sendFile(health: health, peer: peer) }

    func sendClipboardAction(_ peer: PeerCard) -> Action {
        Actions.sendClipboard(health: health, peer: peer, report: clipboard)
    }

    // MARK: - Lifecycle

    func start() {
        guard pollTask == nil else { return }
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.refresh()
                // Cheap rather than instant, as on Linux: everything polled is
                // metadata the agent already holds. Faster while a window is
                // open, because someone is looking.
                let seconds: UInt64 = self.windowVisible ? 2 : 5
                try? await Task.sleep(nanoseconds: seconds * 1_000_000_000)
            }
        }
        offersTask = Task { [weak self] in await self?.watchOffers() }
    }

    /// Ends the streams and the poll. The agent keeps running: it is a
    /// separate service, exactly as `pliweed` outlives the GTK application.
    func shutdown() {
        pollTask?.cancel()
        offersTask?.cancel()
        offerStream?.close()
        pairing?.cancel()
    }

    func refresh() async {
        registration = agent.registration
        openAtLogin = agent.openAtLogin
        let probe: Probe
        do {
            guard case let .status(report) = try await client.request(.status) else {
                throw ControlError.malformed("status")
            }
            status = report
            probe = .answered(report)
        } catch {
            status = nil
            transfers = []
            clipboard = nil
            notifications = nil
            probe = .failed(error.localizedDescription)
        }
        health = ServiceHealth.resolve(
            probe: probe,
            registration: registration,
            secondsSinceEnabled: enabledAt.map { Date().timeIntervalSince($0) }
        )
        guard health.isRunning else { return }
        async let t = client.request(.transfers)
        async let c = client.request(.clipboardStatus)
        async let n = client.request(.notificationsStatus)
        if case let .transfers(list)? = try? await t { transfers = list }
        if case let .clipboard(report)? = try? await c { clipboard = report }
        if case let .notifications(report)? = try? await n { notifications = report }
    }

    // MARK: - Actions

    /// Runs one request, reports its outcome, and refreshes.
    private func perform(_ request: Request, success: String? = nil) {
        Task {
            do {
                let response = try await client.request(request)
                switch response {
                case let .ok(message): show(success ?? message.capitalizedFirst)
                case let .pong(rtt): show("Reachable · \(rtt) ms")
                default: if let success { show(success) }
                }
            } catch {
                show(error.localizedDescription, isError: true)
            }
            await refresh()
        }
    }

    func show(_ text: String, isError: Bool = false) {
        let notice = Notice(text: text, isError: isError)
        self.notice = notice
        Task {
            try? await Task.sleep(nanoseconds: 6_000_000_000)
            if self.notice == notice { self.notice = nil }
        }
    }

    func setGrant(_ capability: String, granted: Bool, for peer: PeerCard) {
        perform(.grant(device: peer.fingerprint, capability: capability, granted: granted))
    }

    func ping(_ peer: PeerCard) { perform(.ping(device: peer.fingerprint)) }

    func unpair(_ peer: PeerCard) {
        perform(.unpair(device: peer.fingerprint), success: "\(peer.name) is no longer paired.")
    }

    func removeFromList(_ peer: PeerCard) {
        perform(.hideRevokedDevice(fingerprint: peer.fingerprint), success: "Removed from the list.")
    }

    func removeAllRevoked() { perform(.hideAllRevokedDevices) }

    func cancel(_ transfer: TransferReport) {
        perform(.cancelTransfer(transfer: transfer.transferId), success: "Cancelled \(transfer.filename).")
    }

    func sendClipboard(to peer: PeerCard, sensitive: Bool = false) {
        guard case let .ready(fingerprint, name) = sendClipboardAction(peer) else { return }
        perform(.clipboardSend(device: fingerprint, sensitive: sensitive), success: "Clipboard sent to \(name).")
    }

    func applyPendingClip(from peer: PeerCard) {
        perform(.clipboardApply(device: peer.fingerprint), success: "Clipboard from \(peer.name) applied.")
    }

    func setClipboardFlag(_ flag: ClipboardFlag, enabled: Bool, for peer: PeerCard) {
        perform(.clipboardPolicy(device: peer.fingerprint, flag: flag, enabled: enabled))
    }

    /// Asks for a file, then offers it to `peer`. The transfer's progress is
    /// what the Files page shows; this follows the stream only to report how
    /// it ended.
    func chooseAndSendFile(to peer: PeerCard) {
        guard case let .ready(fingerprint, name) = sendFileAction(peer) else { return }
        NSApp.activate()
        let panel = NSOpenPanel()
        panel.title = "Send a File to \(name)"
        panel.prompt = "Send"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        send(fileAt: url, to: fingerprint, name: name)
    }

    func send(fileAt url: URL, to fingerprint: String, name: String) {
        let client = client
        Task {
            do {
                let stream = try client.stream(.send(device: fingerprint, path: url.path))
                defer { stream.close() }
                show("Offering \(url.lastPathComponent) to \(name)…")
                await refresh()
                for try await message in stream.messages {
                    switch message {
                    case let .response(.error(text)):
                        show(text, isError: true)
                        return
                    case let .event(.transferProgress(report)) where TransferState.isTerminal(report.state):
                        if report.state == TransferState.completed {
                            show("Sent \(report.filename) to \(name).")
                        } else {
                            show(report.failure ?? "\(report.filename): \(Format.transferState(report)).", isError: true)
                        }
                        await refresh()
                        return
                    default:
                        continue
                    }
                }
            } catch {
                show(error.localizedDescription, isError: true)
            }
            await refresh()
        }
    }

    func revealInFinder(_ transfer: TransferReport) {
        guard let path = transfer.storedAt else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }

    // MARK: - The background service

    func setAgentEnabled(_ enabled: Bool) {
        do {
            try agent.setEnabled(enabled)
            enabledAt = enabled ? Date() : nil
            show(enabled ? "The Pliwee service is starting." : "The Pliwee service is off.")
        } catch {
            show("Could not \(enabled ? "start" : "stop") the Pliwee service: \(error.localizedDescription)", isError: true)
        }
        Task { await refresh() }
    }

    /// Turns the registered service off and on again.
    ///
    /// The fix for an agent that is enabled but not answering. One known
    /// cause is specific to development: launchd records a code requirement
    /// for the agent when it is registered, an ad-hoc signature changes with
    /// every build, and a rebuilt `pliweed` is then refused (`EX_CONFIG`)
    /// until it is registered again. A Developer ID signature does not change
    /// that way.
    func restartAgent() {
        do {
            try agent.setEnabled(false)
            try agent.setEnabled(true)
            enabledAt = Date()
            show("The Pliwee service is restarting.")
        } catch {
            show("Could not restart the Pliwee service: \(error.localizedDescription)", isError: true)
        }
        Task { await refresh() }
    }

    func setOpenAtLogin(_ enabled: Bool) {
        do {
            try agent.setOpenAtLogin(enabled)
        } catch {
            show("Could not change Open at Login: \(error.localizedDescription)", isError: true)
        }
        openAtLogin = agent.openAtLogin
    }

    // MARK: - Pairing

    func beginPairing() {
        pairing?.cancel()
        let session = PairingSession(client: client) { [weak self] in
            Task { await self?.refresh() }
        }
        pairing = session
        session.start()
    }

    func endPairing() {
        pairing?.cancel()
        pairing = nil
    }

    // MARK: - Incoming files

    /// Keeps this app attached as the agent's approval provider.
    ///
    /// With nobody attached the agent declines every offer, which is the
    /// headless default and stays so; attaching gives it someone to ask. When
    /// the connection ends — the agent stopped, or restarted — every prompt
    /// is withdrawn, because a question nobody can act on must not sit in
    /// front of a person with an Accept button under it.
    private func watchOffers() async {
        while !Task.isCancelled {
            if health.isRunning {
                do {
                    let stream = try client.stream(.watchFileOffers)
                    offerStream = stream
                    for try await message in stream.messages {
                        handle(message)
                    }
                } catch {
                    // Reconnect below.
                }
                offerStream = nil
                offers.removeAll()
                approvalUnattended = false
                presentOffers?()
            }
            try? await Task.sleep(nanoseconds: 2_000_000_000)
        }
    }

    private func handle(_ message: StreamMessage) {
        switch message {
        case let .event(.fileApprovalReady(unattended)):
            approvalUnattended = unattended
        case let .event(.fileOfferRequest(offer)):
            guard !offers.contains(where: { $0.transferId == offer.transferId }) else { return }
            offers.append(offer)
            presentOffers?()
        case let .event(.fileOfferWithdrawn(transferId, _)):
            offers.removeAll { $0.transferId == transferId }
            presentOffers?()
        default:
            break
        }
    }

    /// Answers one offer. The full transfer id, matched exactly by the agent.
    func decide(_ offer: FileOfferRequest, accept: Bool) {
        offers.removeAll { $0.transferId == offer.transferId }
        do {
            try offerStream?.send(.fileDecision(transfer: offer.transferId, accept: accept))
        } catch {
            show("Could not answer the offer: \(error.localizedDescription)", isError: true)
        }
        presentOffers?()
    }

    /// Every way out of the prompt that is not the Accept button.
    func declineAll() {
        for offer in offers { decide(offer, accept: false) }
    }
}

/// One pairing window, from the code on screen to its outcome.
@MainActor
@Observable
final class PairingSession {
    enum Phase: Equatable {
        case starting
        case showingCode(payload: String, expiresAt: Date)
        case confirming(name: String, fingerprint: String, short: String)
        case finished(message: String, paired: Bool)
        case failed(String)
    }

    private(set) var phase: Phase = .starting
    private let client: ControlClient
    private var stream: ControlStream?
    private var task: Task<Void, Never>?
    private let onFinish: () -> Void

    init(client: ControlClient, onFinish: @escaping () -> Void) {
        self.client = client
        self.onFinish = onFinish
    }

    func start() {
        task = Task {
            do {
                let stream = try client.stream(.pair(ttlSecs: nil))
                self.stream = stream
                for try await message in stream.messages {
                    switch message {
                    case let .event(.pairingReady(payload, _, expires)):
                        phase = .showingCode(payload: payload, expiresAt: Date().addingTimeInterval(TimeInterval(expires)))
                    case let .event(.confirmRequest(name, _, fingerprint, short)):
                        phase = .confirming(name: name, fingerprint: fingerprint, short: short)
                    case let .event(.finished(status, detail)):
                        phase = .finished(message: Format.pairingOutcome(status: status, detail: detail), paired: status == "paired")
                        stream.close()
                        onFinish()
                        return
                    case let .response(.error(text)):
                        phase = .failed(text)
                        return
                    default:
                        continue
                    }
                }
                if case .finished = phase {} else { phase = .failed("The Pliwee service ended the pairing session.") }
            } catch {
                phase = .failed(error.localizedDescription)
            }
        }
    }

    /// The person's answer to "does this fingerprint match the device?".
    func answer(accept: Bool) {
        do {
            try stream?.send(.confirm(accept: accept))
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    /// Closing the connection is how pairing is cancelled: the agent reads
    /// the hang-up as "operator disconnected" and closes the window.
    func cancel() {
        task?.cancel()
        stream?.close()
    }
}

extension String {
    var capitalizedFirst: String {
        guard let first else { return self }
        return first.uppercased() + dropFirst()
    }
}
