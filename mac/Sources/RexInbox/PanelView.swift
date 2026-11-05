import AppKit
import SwiftUI

enum Tab: Hashable {
    case inbox, agents
}

struct PanelView: View {
    let store: Store
    @State private var tab: Tab

    init(store: Store, tab: Tab = .inbox) {
        self.store = store
        _tab = State(initialValue: tab)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            if store.connected {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        switch tab {
                        case .inbox: InboxView(store: store)
                        case .agents: AgentsView(store: store)
                        }
                    }
                    .padding(.vertical, 6)
                }
                .frame(minHeight: 120, maxHeight: 560)
            } else {
                HubDownView(store: store)
            }
            if let text = store.problem {
                Banner(text: text, color: .red) { store.problem = nil }
            } else if let text = store.note {
                Banner(text: text, color: .secondary) { store.note = nil }
            }
            Divider()
            footer
        }
        .frame(width: 440)
    }

    private var header: some View {
        HStack {
            Text("Rex Inbox").font(.headline)
            Spacer()
            Picker("", selection: $tab) {
                Text(store.waiting > 0 ? "Inbox \(store.waiting)" : "Inbox").tag(Tab.inbox)
                Text("Agents \(store.panes.count)").tag(Tab.agents)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var footer: some View {
        HStack(spacing: 10) {
            Circle().fill(store.connected ? Color.green : Color.gray).frame(width: 7, height: 7)
            Text(store.connected ? "Hub connected" : "Hub not running")
                .font(.caption).foregroundStyle(.secondary)
            Spacer()
            Toggle("Open at Login", isOn: Binding(get: { store.openAtLogin }, set: { _ in store.toggleOpenAtLogin() }))
                .toggleStyle(.checkbox)
                .font(.caption)
            Button("Quit") { NSApp.terminate(nil) }
                .buttonStyle(.link)
                .font(.caption)
                .keyboardShortcut("q")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
    }
}

struct Banner: View {
    let text: String
    let color: Color
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top) {
            Text(text).font(.caption).foregroundStyle(color).textSelection(.enabled)
            Spacer()
            Button(action: dismiss) { Image(systemName: "xmark").font(.caption2) }
                .buttonStyle(.plain).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }
}

struct HubDownView: View {
    let store: Store
    @State private var starting = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("The rex-inbox hub isn't running.").font(.callout)
            Text("It follows your Rex panes and keeps the inbox. Start it as a LaunchAgent (rex-inbox up):")
                .font(.caption).foregroundStyle(.secondary)
            Button(starting ? "Starting…" : "Start the hub") {
                starting = true
                Task {
                    await store.startHub()
                    starting = false
                }
            }
            .disabled(starting)
        }
        .padding(12)
    }
}

struct SectionTitle: View {
    let text: String
    var count: Int? = nil

    var body: some View {
        HStack(spacing: 4) {
            Text(text.uppercased())
            if let count { Text("\(count)").foregroundStyle(.tertiary) }
        }
        .font(.caption2.weight(.semibold))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .padding(.top, 8)
        .padding(.bottom, 2)
    }
}

// MARK: - Inbox

struct InboxView: View {
    let store: Store
    @State private var showFinished = false

    var body: some View {
        if store.alerts.isEmpty && store.messages.isEmpty {
            HStack {
                Image(systemName: "checkmark.circle").foregroundStyle(.green)
                Text("Nothing needs you.").foregroundStyle(.secondary)
            }
            .padding(12)
        }
        if !store.needsYou.isEmpty {
            SectionTitle(text: "Needs you", count: store.needsYou.count)
            ForEach(store.needsYou) { AlertRow(store: store, alert: $0) }
        }
        if !store.messages.isEmpty {
            SectionTitle(text: "Messages", count: store.messages.count)
            ForEach(store.messages) { MessageRow(store: store, message: $0) }
        }
        if !store.failed.isEmpty {
            SectionTitle(text: "Failed", count: store.failed.count)
            ForEach(store.failed) { AlertRow(store: store, alert: $0) }
        }
        if !store.finished.isEmpty {
            Button {
                showFinished.toggle()
            } label: {
                HStack(spacing: 4) {
                    SectionTitle(text: "Finished", count: store.finished.count)
                    Image(systemName: showFinished ? "chevron.down" : "chevron.right")
                        .font(.caption2).foregroundStyle(.secondary).padding(.top, 6)
                }
            }
            .buttonStyle(.plain)
            if showFinished {
                ForEach(store.finished) { AlertRow(store: store, alert: $0) }
            }
        }
    }
}

struct AlertRow: View {
    let store: Store
    let alert: Alert
    @State private var hover = false

    private var color: Color {
        switch alert.category {
        case "needs_you": return .orange
        case "failed": return .red
        default: return .green
        }
    }

    var body: some View {
        let busy = store.busy.contains(alert.id)
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline) {
                Circle().fill(color).frame(width: 7, height: 7)
                Text(alert.headline).font(.callout.weight(.medium))
                Spacer()
                Text(Format.age(alert.createdAt, now: store.now)).font(.caption).foregroundStyle(.secondary)
            }
            Text("#\(alert.id) · \(alert.place) · \(Format.tilde(alert.cwd))")
                .font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                .padding(.leading, 15)
            if !alert.what.isEmpty {
                Text(alert.what)
                    .font(.system(.caption, design: .monospaced))
                    .lineLimit(3)
                    .textSelection(.enabled)
                    .padding(.leading, 15)
            }
            HStack(spacing: 8) {
                if alert.canAnswer {
                    Button("Approve") { Task { await store.approve(alert.id, seen: alert.msg) } }
                        .buttonStyle(.borderedProminent).controlSize(.small)
                    Button("Deny") { Task { await store.deny(alert.id, seen: alert.msg) } }
                        .controlSize(.small)
                }
                Button("Open") { Task { await store.open(alert.id) } }
                    .controlSize(.small)
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                if alert.category == "needs_you" {
                    Button("Snooze 1h") { Task { await store.snooze(alert.id) } }
                        .buttonStyle(.link).font(.caption)
                }
                Button("Done") { Task { await store.done(alert.id) } }
                    .buttonStyle(.link).font(.caption)
            }
            .padding(.leading, 15)
            .padding(.top, 2)
            .disabled(busy)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(hover ? Color.primary.opacity(0.05) : .clear)
        .onHover { hover = $0 }
    }
}

struct MessageRow: View {
    let store: Store
    let message: Message
    @State private var draft = ""
    @State private var expanded = false

    var body: some View {
        let busy = store.busy.contains(message.id)
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline) {
                Image(systemName: message.status == "unread" ? "envelope.badge" : "envelope.open")
                    .foregroundStyle(message.status == "unread" ? Color.accentColor : .secondary)
                Text(message.from).font(.callout.weight(.medium))
                Text(message.firstLine).font(.callout).lineLimit(1)
                Spacer()
                Text(Format.age(message.createdAt, now: store.now)).font(.caption).foregroundStyle(.secondary)
            }
            Text(message.body)
                .font(.caption)
                .lineLimit(expanded ? nil : 4)
                .textSelection(.enabled)
                .padding(.leading, 22)
                .onTapGesture { expanded.toggle() }
            HStack(spacing: 6) {
                TextField("Reply to #\(message.id)…", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...4)
                    .controlSize(.small)
                    .onSubmit(send)
                Button("Send", action: send)
                    .controlSize(.small)
                    .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
            }
            .padding(.leading, 22)
            HStack(spacing: 8) {
                if message.fromBlock != nil {
                    Button("Open") { Task { await store.open(message.id) } }.buttonStyle(.link).font(.caption)
                }
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                Button("Done") { Task { await store.done(message.id) } }.buttonStyle(.link).font(.caption)
            }
            .padding(.leading, 22)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        Task { await store.reply(message.id, text) }
    }
}

// MARK: - Agents

struct AgentsView: View {
    let store: Store

    private var sessions: [(String, [Pane])] {
        let groups = Dictionary(grouping: store.panes, by: \.session)
        return groups.keys.sorted().map { ($0, groups[$0]!) }
    }

    var body: some View {
        if store.panes.isEmpty {
            Text("No agents are running in Rex.").foregroundStyle(.secondary).padding(12)
        }
        ForEach(sessions, id: \.0) { session, panes in
            SectionTitle(text: session, count: panes.count)
            ForEach(panes) { PaneRow(store: store, pane: $0) }
        }
    }
}

struct PaneRow: View {
    let store: Store
    let pane: Pane
    @State private var hover = false
    @State private var composing = false
    @State private var draft = ""

    private var color: Color {
        switch pane.state {
        case "blocked": return .orange
        case "error": return .red
        case "working": return .blue
        case "done": return .green
        default: return .gray
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Circle().fill(color).frame(width: 7, height: 7)
                Text(pane.name).font(.callout.weight(.medium))
                Text(Format.appName(pane.app)).font(.caption).foregroundStyle(.secondary)
                Text(stateText).font(.caption).foregroundStyle(color)
                Spacer()
                if hover {
                    Button {
                        composing.toggle()
                    } label: {
                        Image(systemName: "envelope")
                    }
                    .buttonStyle(.borderless)
                    .help("Message this agent")
                }
                Text(Format.age(pane.since, now: store.now)).font(.caption).foregroundStyle(.secondary)
            }
            Text(Format.tilde(pane.cwd))
                .font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                .padding(.leading, 13)
            if let what = [pane.title, pane.msg].compactMap({ $0 }).first, !what.isEmpty {
                Text(what).font(.caption).lineLimit(2).padding(.leading, 13)
            }
            if composing {
                HStack(spacing: 6) {
                    TextField("Message \(pane.address)…", text: $draft, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(1...4)
                        .controlSize(.small)
                        .onSubmit(send)
                    Button("Send", action: send)
                        .controlSize(.small)
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                .padding(.leading, 13)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 5)
        .contentShape(Rectangle())
        .background(hover ? Color.primary.opacity(0.05) : .clear)
        .onHover { hover = $0 }
        .onTapGesture { Task { await store.open(pane) } }
        .help("Open \(pane.address) in Rex")
    }

    private var stateText: String {
        var s = pane.state ?? ""
        if let k = pane.kind { s += " (\(k))" }
        if pane.workingChildren > 0 { s += " +\(pane.workingChildren)" }
        return s
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        composing = false
        Task { await store.send(to: pane, text) }
    }
}
