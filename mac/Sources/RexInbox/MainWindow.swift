import AppKit
import SwiftUI

enum SidebarItem: Hashable {
    case inbox(InboxFilter)
    case agents
}

/// The full window: a sidebar of views, and room to read and answer.
struct MainWindow: View {
    let store: Store
    @State private var selection: SidebarItem? = .inbox(.all)
    @Environment(\.openWindow) private var openWindow

    init(store: Store, selection: SidebarItem = .inbox(.all)) {
        self.store = store
        _selection = State(initialValue: selection)
    }

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Section("Inbox") {
                    row(.inbox(.all), "Everything", "tray.full", store.alerts.count + store.messages.count)
                    row(.inbox(.needsYou), "Needs You", "hand.raised", store.needsYou.count)
                    row(.inbox(.messages), "Messages", "envelope", store.messages.count)
                    row(.inbox(.failed), "Failed", "xmark.octagon", store.failed.count)
                    row(.inbox(.finished), "Finished", "checkmark.circle", store.finished.count)
                }
                Section("Rex") {
                    row(.agents, "Agents", "person.2", store.panes.count)
                }
            }
            .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 280)
            .safeAreaInset(edge: .bottom) { status }
        } detail: {
            VStack(spacing: 0) {
                if store.connected {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 12) {
                            switch selection ?? .inbox(.all) {
                            case .inbox(let filter): InboxList(store: store, filter: filter)
                            case .agents: AgentsList(store: store)
                            }
                        }
                        .frame(maxWidth: 860, alignment: .leading)
                        .padding(.horizontal, 24)
                        .padding(.vertical, 18)
                        .frame(maxWidth: .infinity)
                    }
                } else {
                    HubDownView(store: store)
                    Spacer()
                }
                Banners(store: store)
            }
            .navigationTitle(title)
            .navigationSubtitle(store.summary)
            .toolbar {
                ToolbarItem {
                    Button {
                        openWindow(id: "new-agent")
                    } label: {
                        Label("New Agent", systemImage: "plus")
                    }
                    .help("Start an agent, here or on another Rex host")
                    .disabled(!store.connected)
                }
            }
        }
        .frame(minWidth: 780, minHeight: 520)
        // In the Dock and the app switcher while the window is open.
        .onAppear { NSApp.setActivationPolicy(.regular) }
        .onDisappear { NSApp.setActivationPolicy(.accessory) }
    }

    private var title: String {
        switch selection ?? .inbox(.all) {
        case .inbox(.all): return "Everything"
        case .inbox(.needsYou): return "Needs You"
        case .inbox(.messages): return "Messages"
        case .inbox(.failed): return "Failed"
        case .inbox(.finished): return "Finished"
        case .agents: return "Agents"
        }
    }

    private func row(_ item: SidebarItem, _ name: String, _ symbol: String, _ count: Int) -> some View {
        Label(name, systemImage: symbol)
            .badge(count)
            .tag(item)
    }

    private var status: some View {
        HStack(spacing: 8) {
            Circle().fill(store.connected ? Color.green : Color.gray).frame(width: 8, height: 8)
            Text(store.connected ? "Hub connected" : "Hub not running").font(.callout).foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}
