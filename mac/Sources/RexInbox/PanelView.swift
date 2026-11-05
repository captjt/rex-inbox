import AppKit
import SwiftUI

enum Tab: Hashable {
    case inbox, agents
}

/// The menu bar panel: a quick look, with a button to open the full window.
struct PanelView: View {
    let store: Store
    @State private var tab: Tab
    @Environment(\.openWindow) private var openWindow

    init(store: Store, tab: Tab = .inbox) {
        self.store = store
        _tab = State(initialValue: tab)
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if store.connected {
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        switch tab {
                        case .inbox: InboxList(store: store)
                        case .agents: AgentsList(store: store)
                        }
                    }
                    .padding(14)
                }
                .frame(minHeight: 220, maxHeight: 680)
            } else {
                HubDownView(store: store)
            }
            Banners(store: store)
            Divider()
            footer
        }
        .frame(width: 540)
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Rex Inbox").font(.title3.weight(.semibold))
                Text(store.summary).font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            Picker("", selection: $tab) {
                Text("Inbox").tag(Tab.inbox)
                Text("Agents").tag(Tab.agents)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            Button {
                openWindow(id: "main")
                NSApp.activate()
            } label: {
                Image(systemName: "macwindow")
            }
            .buttonStyle(.borderless)
            .font(.title3)
            .help("Open Rex Inbox in a window")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var footer: some View {
        HStack(spacing: 12) {
            Circle().fill(store.connected ? Color.green : Color.gray).frame(width: 8, height: 8)
            Text(store.connected ? "Hub connected" : "Hub not running")
                .font(.callout).foregroundStyle(.secondary)
            Spacer()
            Toggle("Open at Login", isOn: Binding(get: { store.openAtLogin }, set: { _ in store.toggleOpenAtLogin() }))
                .toggleStyle(.checkbox)
                .font(.callout)
            Button("Quit") { NSApp.terminate(nil) }
                .keyboardShortcut("q")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}
