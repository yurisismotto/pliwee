// The main window: a sidebar of pages and the page itself.

import PliweeKit
import SwiftUI

struct MainWindow: View {
    @Bindable var model: AppModel

    var body: some View {
        NavigationSplitView {
            List(Page.allCases, selection: Binding($model.page)) { page in
                Label(page.title, systemImage: page.symbol)
                    .tag(page)
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
            .safeAreaInset(edge: .bottom) {
                ServiceBadge(health: model.health)
                    .padding(12)
            }
        } detail: {
            VStack(spacing: 0) {
                page
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if let notice = model.notice {
                    NoticeBar(notice: notice)
                }
            }
            .navigationTitle(model.page.title)
        }
        .tint(.pliweeBlue)
        .sheet(item: Binding(
            get: { model.pairing.map(PairingSheetItem.init) },
            set: { if $0 == nil { model.endPairing() } }
        )) { item in
            PairingView(session: item.session) { model.endPairing() }
        }
    }

    @ViewBuilder
    private var page: some View {
        switch model.page {
        case .overview: OverviewView(model: model)
        case .devices: DevicesView(model: model)
        case .files: FilesView(model: model)
        case .clipboard: ClipboardView(model: model)
        case .settings: SettingsView(model: model)
        }
    }
}

private struct PairingSheetItem: Identifiable {
    let session: PairingSession
    var id: ObjectIdentifier { ObjectIdentifier(session) }
}

/// Where the service stands, in the sidebar: an icon, a word and a colour —
/// never the colour alone.
struct ServiceBadge: View {
    let health: ServiceHealth

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: health.symbol)
                .foregroundStyle(health.color)
            VStack(alignment: .leading, spacing: 1) {
                Text(health.title).font(.callout.weight(.semibold))
                Text("Pliwee service").font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .combine)
    }
}

struct NoticeBar: View {
    let notice: Notice

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: notice.isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                .foregroundStyle(notice.isError ? Color.token(DesignTokens.Status.error) : Color.token(DesignTokens.Status.success))
            Text(notice.text).lineLimit(2)
            Spacer()
        }
        .font(.callout)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
        .transition(.move(edge: .bottom))
    }
}

/// A page section with a title, in the macOS grouped-form style.
struct PageScroll<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                content
            }
            .padding(24)
            .frame(maxWidth: 760, alignment: .leading)
        }
    }
}

struct Card<Content: View>: View {
    let title: String?
    @ViewBuilder var content: Content

    init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let title {
                Text(title).font(.headline)
            }
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.separator.opacity(0.6)))
    }
}

/// A setting: what it does on the left, its switch on the right, every row
/// aligned the same way.
struct SwitchRow: View {
    let title: String
    var caption: String? = nil
    let isOn: Binding<Bool>

    var body: some View {
        HStack(alignment: .center, spacing: 16) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                if let caption {
                    Text(caption)
                        .font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            Toggle(title, isOn: isOn)
                .labelsHidden()
                .toggleStyle(.switch)
        }
        .accessibilityElement(children: .combine)
    }
}

/// A label and a value on one row.
struct FactRow: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label).foregroundStyle(.secondary).frame(width: 150, alignment: .leading)
            Text(value)
                .font(monospaced ? .body.monospaced() : .body)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }
}

/// What to show when the service is not answering: the reason, and the one
/// thing that fixes it.
struct ServiceUnavailableView: View {
    let model: AppModel

    var body: some View {
        ContentUnavailableView {
            Label(model.health.title, systemImage: model.health.symbol)
        } description: {
            Text(model.health.headline)
        } actions: {
            switch model.health {
            case .off:
                Button("Turn On Pliwee Service") { model.setAgentEnabled(true) }
                    .buttonStyle(.borderedProminent)
                    .disabled(model.registration == .notFound)
            case .needsApproval:
                Button("Open Login Items Settings…") { model.agent.openLoginItemsSettings() }
            case .notResponding:
                HStack {
                    Button("Restart Service") { model.restartAgent() }
                    Button("Show Log in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([model.paths.logFile])
                    }
                }
            case .unreachable:
                Button("Show Log in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([model.paths.logFile])
                }
            default:
                EmptyView()
            }
        }
    }
}

extension ServiceHealth {
    var color: Color {
        switch self {
        case let .running(connected, _):
            return connected > 0 ? .token(DesignTokens.Status.connected) : .token(DesignTokens.Status.available)
        case .starting: return .token(DesignTokens.Status.transferring)
        case .off, .needsApproval: return .secondary
        case .notResponding, .unreachable: return .token(DesignTokens.Status.warning)
        }
    }
}

extension PeerLink {
    var color: Color {
        switch self {
        case .connected: return .token(DesignTokens.Status.connected)
        case .stale: return .token(DesignTokens.Status.stale)
        case .offline: return .secondary
        }
    }

    var symbol: String {
        switch self {
        case .connected: return "circle.fill"
        case .stale: return "exclamationmark.circle"
        case .offline: return "circle"
        }
    }
}
