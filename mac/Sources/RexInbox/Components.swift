import SwiftUI

// The cards the panel and the window share.

extension Alert {
    var color: Color {
        switch category {
        case "needs_you": return .orange
        case "failed": return .red
        default: return .green
        }
    }

    var symbol: String {
        switch (category, kind) {
        case ("failed", _): return "xmark.octagon.fill"
        case ("finished", _): return "checkmark.circle.fill"
        case (_, "permission"): return "hand.raised.fill"
        case (_, "question"): return "questionmark.bubble.fill"
        case (_, "auth"): return "key.fill"
        default: return "exclamationmark.circle.fill"
        }
    }
}

func stateColor(_ state: String?) -> Color {
    switch state {
    case "blocked": return .orange
    case "error": return .red
    case "working": return .blue
    case "done": return .green
    default: return .gray
    }
}

/// A rounded card with an optional colored edge.
struct Card<Content: View>: View {
    var tint: Color? = nil
    @ViewBuilder var content: Content
    @State private var hover = false

    var body: some View {
        content
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.primary.opacity(hover ? 0.075 : 0.045))
            )
            .overlay(alignment: .leading) {
                if let tint {
                    RoundedRectangle(cornerRadius: 1.5)
                        .fill(tint)
                        .frame(width: 3)
                        .padding(.vertical, 12)
                }
            }
            .onHover { hover = $0 }
    }
}

struct SectionHeader: View {
    let title: String
    var count: Int? = nil

    var body: some View {
        HStack(spacing: 6) {
            Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
            if let count {
                Text("\(count)")
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(Capsule().fill(Color.primary.opacity(0.08)))
            }
        }
        .padding(.top, 6)
    }
}

struct StatePill: View {
    let state: String?
    var kind: String? = nil

    var body: some View {
        let color = stateColor(state)
        HStack(spacing: 4) {
            Circle().fill(color).frame(width: 6, height: 6)
            Text(kind.map { "\(state ?? "") · \($0)" } ?? (state ?? "unknown"))
        }
        .font(.caption.weight(.semibold))
        .foregroundStyle(color)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(Capsule().fill(color.opacity(0.15)))
    }
}

struct EmptyState: View {
    let symbol: String
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: symbol).font(.system(size: 34)).foregroundStyle(.green)
            Text(title).font(.title3.weight(.semibold))
            Text(detail).font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 36)
    }
}

/// `#18 · solar-isle/alpha · ~/git/app`
struct MetaLine: View {
    let parts: [String]

    var body: some View {
        Text(parts.filter { !$0.isEmpty }.joined(separator: "  ·  "))
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .truncationMode(.middle)
    }
}

// MARK: - Alerts

struct AlertCard: View {
    let store: Store
    let alert: Alert

    var body: some View {
        let busy = store.busy.contains(alert.id)
        Card(tint: alert.color) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: alert.symbol).foregroundStyle(alert.color)
                    Text(alert.headline).font(.headline)
                    Spacer(minLength: 8)
                    Text(Format.age(alert.createdAt, now: store.now) + " ago")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                MetaLine(parts: ["#\(alert.id)", alert.place, Format.tilde(alert.cwd)])
                if !alert.what.isEmpty {
                    Text(alert.what)
                        .font(.system(.callout, design: .monospaced))
                        .textSelection(.enabled)
                        .lineLimit(8)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(0.06)))
                }
                HStack(spacing: 8) {
                    if alert.canAnswer {
                        Button {
                            Task { await store.approve(alert.id, seen: alert.msg) }
                        } label: {
                            Label("Approve", systemImage: "checkmark")
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.green)
                        Button(role: .destructive) {
                            Task { await store.deny(alert.id, seen: alert.msg) }
                        } label: {
                            Label("Deny", systemImage: "xmark")
                        }
                    }
                    Button {
                        Task { await store.open(alert.id) }
                    } label: {
                        Label("Open in Rex", systemImage: "arrow.up.forward.app")
                    }
                    if busy { ProgressView().controlSize(.small) }
                    Spacer()
                    if alert.category == "needs_you" {
                        Menu {
                            Button("Snooze 1 hour") { Task { await store.snooze(alert.id, "1h") } }
                            Button("Snooze 4 hours") { Task { await store.snooze(alert.id, "4h") } }
                            Button("Snooze until tomorrow") { Task { await store.snooze(alert.id, "1d") } }
                            Divider()
                            Button("Mark Done") { Task { await store.done(alert.id) } }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .fixedSize()
                        .help("Snooze or mark done")
                    } else {
                        Button("Done") { Task { await store.done(alert.id) } }
                    }
                }
                .padding(.top, 2)
                .disabled(busy)
            }
        }
    }
}

// MARK: - Messages

struct MessageCard: View {
    let store: Store
    let message: Message
    @State private var draft = ""
    @State private var expanded = false

    private var long: Bool { message.body.count > 400 || message.body.split(separator: "\n").count > 6 }

    var body: some View {
        let busy = store.busy.contains(message.id)
        let unread = message.status == "unread"
        Card(tint: unread ? .accentColor : nil) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: unread ? "envelope.badge.fill" : "envelope.open")
                        .foregroundStyle(unread ? Color.accentColor : .secondary)
                    Text(message.from).font(.headline)
                    if let r = message.replyTo {
                        Text("replying to #\(r)").font(.subheadline).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    Text("#\(message.id)  ·  " + Format.age(message.createdAt, now: store.now) + " ago")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                if let subject = message.subject, !subject.isEmpty {
                    Text(subject).font(.body.weight(.semibold))
                }
                Text(message.body)
                    .font(.body)
                    .textSelection(.enabled)
                    .lineLimit(expanded ? nil : 6)
                    .fixedSize(horizontal: false, vertical: true)
                if long {
                    Button(expanded ? "Show less" : "Show more") { expanded.toggle() }
                        .buttonStyle(.link)
                }
                HStack(alignment: .bottom, spacing: 8) {
                    TextField("Reply…", text: $draft, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(1...6)
                        .onSubmit(send)
                    Button("Send", action: send)
                        .buttonStyle(.borderedProminent)
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
                }
                .padding(.top, 2)
                HStack(spacing: 8) {
                    if message.fromBlock != nil {
                        Button {
                            Task { await store.open(message.id) }
                        } label: {
                            Label("Open in Rex", systemImage: "arrow.up.forward.app")
                        }
                    }
                    if busy { ProgressView().controlSize(.small) }
                    Spacer()
                    Button("Done") { Task { await store.done(message.id) } }
                }
                .disabled(busy)
            }
        }
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        Task { await store.reply(message.id, text) }
    }
}

// MARK: - Agents

struct AgentCard: View {
    let store: Store
    let pane: Pane
    @State private var composing = false
    @State private var draft = ""

    private var activity: String? {
        [pane.title, pane.msg].compactMap { $0 }.first { !$0.isEmpty }
    }

    var body: some View {
        Card(tint: pane.state == "blocked" || pane.state == "error" ? stateColor(pane.state) : nil) {
            VStack(alignment: .leading, spacing: 7) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(pane.name).font(.headline)
                    Text(Format.appName(pane.app)).font(.subheadline).foregroundStyle(.secondary)
                    StatePill(state: pane.state, kind: pane.kind)
                    if pane.workingChildren > 0 {
                        Text("+\(pane.workingChildren) subagent\(pane.workingChildren == 1 ? "" : "s")")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    Text(Format.age(pane.since, now: store.now)).font(.subheadline).foregroundStyle(.secondary)
                }
                HStack(spacing: 8) {
                    MetaLine(parts: [Format.tilde(pane.cwd)])
                    Spacer(minLength: 8)
                    Button {
                        Task { await store.open(pane) }
                    } label: {
                        Label("Open in Rex", systemImage: "arrow.up.forward.app")
                    }
                    Button {
                        composing.toggle()
                    } label: {
                        Label("Message", systemImage: "envelope")
                    }
                }
                .controlSize(.small)
                if let activity {
                    Text(activity).font(.callout).lineLimit(2)
                }
                if composing {
                    HStack(alignment: .bottom, spacing: 8) {
                        TextField("Message \(pane.address)…", text: $draft, axis: .vertical)
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(1...6)
                            .onSubmit(send)
                        Button("Send", action: send)
                            .buttonStyle(.borderedProminent)
                            .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
            }
        }
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        composing = false
        Task { await store.send(to: pane, text) }
    }
}

// MARK: - Lists

enum InboxFilter: Hashable {
    case all, needsYou, messages, failed, finished
}

/// Inbox items as cards, in sections when showing everything.
struct InboxList: View {
    let store: Store
    var filter: InboxFilter = .all
    @State private var showFinished = false

    var body: some View {
        switch filter {
        case .all:
            if store.alerts.isEmpty && store.messages.isEmpty {
                EmptyState(symbol: "checkmark.circle", title: "Nothing needs you",
                           detail: "Agents that are blocked, failed, or finished show up here, with messages to you.")
            }
            if !store.needsYou.isEmpty {
                SectionHeader(title: "Needs you", count: store.needsYou.count)
                ForEach(store.needsYou) { AlertCard(store: store, alert: $0) }
            }
            if !store.messages.isEmpty {
                SectionHeader(title: "Messages", count: store.messages.count)
                ForEach(store.messages) { MessageCard(store: store, message: $0) }
            }
            if !store.failed.isEmpty {
                SectionHeader(title: "Failed", count: store.failed.count)
                ForEach(store.failed) { AlertCard(store: store, alert: $0) }
            }
            if !store.finished.isEmpty {
                Button {
                    withAnimation(.easeInOut(duration: 0.15)) { showFinished.toggle() }
                } label: {
                    HStack(spacing: 6) {
                        SectionHeader(title: "Finished", count: store.finished.count)
                        Image(systemName: showFinished ? "chevron.down" : "chevron.right")
                            .font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.top, 6)
                        Spacer()
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if showFinished {
                    ForEach(store.finished) { AlertCard(store: store, alert: $0) }
                }
            }
        case .needsYou:
            alerts(store.needsYou, empty: "Nothing is waiting on you.")
        case .failed:
            alerts(store.failed, empty: "No agent has failed.")
        case .finished:
            alerts(store.finished, empty: "Nothing finished is waiting to be looked at.")
        case .messages:
            if store.messages.isEmpty {
                EmptyState(symbol: "envelope.open", title: "No messages", detail: "Agents' messages to you show up here.")
            }
            ForEach(store.messages) { MessageCard(store: store, message: $0) }
        }
    }

    @ViewBuilder
    private func alerts(_ list: [Alert], empty: String) -> some View {
        if list.isEmpty {
            EmptyState(symbol: "checkmark.circle", title: "All clear", detail: empty)
        }
        ForEach(list) { AlertCard(store: store, alert: $0) }
    }
}

struct AgentsList: View {
    let store: Store

    private var sessions: [(String, [Pane])] {
        let groups = Dictionary(grouping: store.panes, by: \.session)
        return groups.keys.sorted().map { ($0, groups[$0]!) }
    }

    var body: some View {
        if store.panes.isEmpty {
            EmptyState(symbol: "person.2", title: "No agents running",
                       detail: "Claude Code and Codex panes in Rex show up here.")
        }
        ForEach(sessions, id: \.0) { session, panes in
            SectionHeader(title: session, count: panes.count)
            ForEach(panes) { AgentCard(store: store, pane: $0) }
        }
    }
}

struct Banners: View {
    let store: Store

    var body: some View {
        if let text = store.problem {
            banner(text, color: .red, symbol: "exclamationmark.triangle.fill") { store.problem = nil }
        } else if let text = store.note {
            banner(text, color: .secondary, symbol: "info.circle") { store.note = nil }
        }
    }

    private func banner(_ text: String, color: Color, symbol: String, dismiss: @escaping () -> Void) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: symbol).foregroundStyle(color)
            Text(text).font(.callout).foregroundStyle(color).textSelection(.enabled)
            Spacer()
            Button(action: dismiss) { Image(systemName: "xmark") }
                .buttonStyle(.borderless).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 9)
    }
}

struct HubDownView: View {
    let store: Store
    @State private var starting = false

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "bolt.horizontal.circle").font(.system(size: 34)).foregroundStyle(.secondary)
            Text("The rex-inbox hub isn't running").font(.title3.weight(.semibold))
            Text("It follows your Rex panes and keeps the inbox. Start it as a LaunchAgent, the same as rex-inbox up.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button(starting ? "Starting…" : "Start the Hub") {
                starting = true
                Task {
                    await store.startHub()
                    starting = false
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(starting)
        }
        .frame(maxWidth: .infinity)
        .padding(28)
    }
}
