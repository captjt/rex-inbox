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
        case (_, "startup"): return "hourglass"
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

/// A rounded card.
struct Card<Content: View>: View {
    @ViewBuilder var content: Content
    @State private var hover = false

    var body: some View {
        content
            .padding(.vertical, 10)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.primary.opacity(hover ? 0.075 : 0.045))
            )
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
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let busy = store.busy.contains(alert.id)
        Card {
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: alert.symbol).foregroundStyle(alert.color)
                    Text(alert.headline).font(.headline)
                    Spacer(minLength: 8)
                    Text(Format.age(alert.createdAt, now: store.now) + " ago")
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                HStack(spacing: 8) {
                    MetaLine(parts: ["#\(alert.id)", alert.place, Format.tilde(alert.cwd)])
                    Spacer(minLength: 8)
                    // Without Approve and Deny, the actions fit on this line.
                    if !alert.canAnswer {
                        actions(busy: busy).controlSize(.small)
                    }
                }
                if !alert.what.isEmpty {
                    if alert.category == "needs_you" {
                        Text(alert.what)
                            .font(.system(.callout, design: .monospaced))
                            .textSelection(.enabled)
                            .lineLimit(8)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 7)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(0.06)))
                    } else {
                        Text(alert.what).font(.callout).textSelection(.enabled).lineLimit(4)
                    }
                }
                if alert.canAnswer {
                    HStack(spacing: 8) {
                        ForEach(alert.decisions, id: \.action) { decision($0) }
                        if alert.canPreview {
                            Button {
                                Task {
                                    if await store.preview(alert) {
                                        openWindow(id: "preview")
                                        NSApp.activate()
                                    }
                                }
                            } label: {
                                Label("Preview", systemImage: "doc.text.magnifyingglass")
                            }
                        }
                        Spacer()
                        actions(busy: busy)
                    }
                    .padding(.top, 2)
                    .disabled(busy)
                }
            }
        }
    }

    /// Approve (or Post), and Deny (or Dismiss): the hub names them.
    @ViewBuilder
    private func decision(_ a: Answer) -> some View {
        if a.action == "approve" {
            Button {
                Task { await store.answer(a.action, alert.id, seen: alert.msg) }
            } label: {
                Label(a.label, systemImage: "checkmark")
            }
            .buttonStyle(.borderedProminent)
            .tint(.green)
        } else {
            Button(role: .destructive) {
                Task { await store.answer(a.action, alert.id, seen: alert.msg) }
            } label: {
                Label(a.label, systemImage: "xmark")
            }
        }
    }

    /// Open, and Snooze/Done (a menu for what needs you, a button otherwise).
    @ViewBuilder
    private func actions(busy: Bool) -> some View {
        HStack(spacing: 8) {
            if busy { ProgressView().controlSize(.small) }
            Button {
                Task { await store.open(alert.id) }
            } label: {
                Label("Open in Rex", systemImage: "arrow.up.forward.app")
            }
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
        .disabled(busy)
    }
}

// MARK: - Messages

struct MessageCard: View {
    let store: Store
    let message: Message
    @State private var draft = ""
    @State private var expanded = false
    @State private var replying = false
    @FocusState private var focused: Bool

    private var long: Bool { message.body.count > 400 || message.body.split(separator: "\n").count > 6 }

    var body: some View {
        let busy = store.busy.contains(message.id)
        let unread = message.status == "unread"
        Card {
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: message.preview != nil ? "globe" : unread ? "envelope.badge.fill" : "envelope.open")
                        .foregroundStyle(unread || message.preview?.live == true ? Color.accentColor : .secondary)
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
                    .lineLimit(expanded ? nil : 4)
                    .fixedSize(horizontal: false, vertical: true)
                if long {
                    Button(expanded ? "Show less" : "Show more") { expanded.toggle() }
                        .buttonStyle(.link)
                }
                if let p = message.preview {
                    previewRow(p, busy: busy)
                }
                if replying {
                    HStack(alignment: .bottom, spacing: 8) {
                        TextField("Reply to \(message.from)…", text: $draft, axis: .vertical)
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(1...6)
                            .focused($focused)
                            .onSubmit(send)
                        Button("Send", action: send)
                            .buttonStyle(.borderedProminent)
                            .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
                    }
                    .padding(.top, 2)
                }
                HStack(spacing: 8) {
                    if !replying {
                        Button {
                            replying = true
                            focused = true
                        } label: {
                            Label("Reply", systemImage: "arrowshape.turn.up.left")
                        }
                        .buttonStyle(.borderedProminent)
                    }
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

    /// Open, and the agent's answers, while the preview is served.
    @ViewBuilder
    private func previewRow(_ p: Preview, busy: Bool) -> some View {
        if p.live {
            HStack(spacing: 8) {
                Button {
                    if let url = URL(string: p.url) { NSWorkspace.shared.open(url) }
                } label: {
                    Label("Open", systemImage: "safari")
                }
                .buttonStyle(.borderedProminent)
                .help(p.url)
                if let said = p.answer {
                    Label("You said “\(said)”", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green).font(.callout.weight(.medium))
                } else {
                    ForEach(p.options ?? [], id: \.self) { option in
                        Button(option) { Task { await store.reply(message.id, option) } }
                    }
                }
                Spacer()
                Text("\(p.waiting ? "Wants your review" : "Preview") · port \(String(p.port))").font(.caption).foregroundStyle(.secondary)
            }
            .disabled(busy)
        } else {
            Text("Preview stopped · \(p.stopped ?? "no longer served")").font(.caption).foregroundStyle(.secondary)
        }
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        replying = false
        Task { await store.reply(message.id, text) }
    }
}

// MARK: - Agents

struct AgentCard: View {
    let store: Store
    let pane: Pane
    /// Name the pane by `session/pane`, where it isn't under its session's header.
    var showPlace = false
    @State private var composing = false
    @State private var draft = ""

    private var activity: String? {
        [pane.title, pane.msg].compactMap { $0 }.first { !$0.isEmpty }
    }

    var body: some View {
        Card {
            VStack(alignment: .leading, spacing: 7) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(showPlace ? pane.address : pane.name).font(.headline)
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
                           detail: store.running.isEmpty
                               ? "Agents that are blocked, failed, or finished show up here, with messages to you."
                               : "Your agents are below. You'll hear when one needs you.")
            }
            if store.needsYouCount > 0 {
                SectionHeader(title: "Needs you", count: store.needsYouCount)
                ForEach(store.needsYou) { AlertCard(store: store, alert: $0) }
                ForEach(store.reviews) { MessageCard(store: store, message: $0) }
            }
            if !store.mail.isEmpty {
                SectionHeader(title: "Messages", count: store.mail.count)
                ForEach(store.mail) { MessageCard(store: store, message: $0) }
            }
            if !store.failed.isEmpty {
                SectionHeader(title: "Failed", count: store.failed.count)
                ForEach(store.failed) { AlertCard(store: store, alert: $0) }
            }
            if !store.running.isEmpty {
                SectionHeader(title: "Running", count: store.running.count)
                ForEach(store.running) { AgentCard(store: store, pane: $0, showPlace: true) }
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
            if store.reviews.isEmpty {
                alerts(store.needsYou, empty: "Nothing is waiting on you.")
            } else {
                ForEach(store.needsYou) { AlertCard(store: store, alert: $0) }
            }
            ForEach(store.reviews) { MessageCard(store: store, message: $0) }
        case .failed:
            alerts(store.failed, empty: "No agent has failed.")
        case .finished:
            alerts(store.finished, empty: "Nothing finished is waiting to be looked at.")
        case .messages:
            if store.mail.isEmpty {
                EmptyState(symbol: "envelope.open", title: "No messages", detail: "Agents' messages to you show up here.")
            }
            ForEach(store.mail) { MessageCard(store: store, message: $0) }
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

    /// This Mac's sessions, then other hosts', each as `host · session`.
    private var sessions: [(String, [Pane])] {
        let groups = Dictionary(grouping: store.panes, by: \.group)
        return groups.keys
            .sorted { a, b in
                let (la, lb) = (groups[a]!.first!.isLocal, groups[b]!.first!.isLocal)
                return la != lb ? la : a < b
            }
            .map { ($0, groups[$0]!) }
    }

    var body: some View {
        if store.panes.isEmpty {
            EmptyState(symbol: "person.2", title: "No agents running",
                       detail: "Claude Code and Codex panes in Rex show up here.")
        }
        ForEach(sessions, id: \.0) { session, panes in
            SessionHeader(store: store, title: session, panes: panes)
            ForEach(panes) { AgentCard(store: store, pane: $0) }
        }
    }
}

/// A session's header in the Agents list, with a pencil to rename it.
struct SessionHeader: View {
    let store: Store
    let title: String
    let panes: [Pane]
    @State private var editing = false
    @State private var draft = ""
    @State private var error: String?
    @State private var saving = false
    @FocusState private var focused: Bool

    var body: some View {
        if editing, let pane = panes.first {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    if !pane.isLocal {
                        Text("\(pane.host) ·").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                    }
                    TextField("Session name", text: $draft)
                        .textFieldStyle(.roundedBorder)
                        .frame(maxWidth: 260)
                        .focused($focused)
                        .onSubmit { save(pane) }
                        .onExitCommand { editing = false }
                    Button("Rename") { save(pane) }
                        .buttonStyle(.borderedProminent)
                        .disabled(saving || draft.trimmingCharacters(in: .whitespaces).isEmpty)
                    Button("Cancel") { editing = false }
                    Spacer()
                }
                .controlSize(.small)
                if let error {
                    Text(error).font(.caption).foregroundStyle(.red)
                }
            }
            .padding(.top, 6)
            .onAppear { focused = true }
        } else {
            HStack(spacing: 6) {
                SectionHeader(title: title, count: panes.count)
                Button {
                    draft = panes.first?.sessionLabel ?? ""
                    error = nil
                    editing = true
                } label: {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help("Rename this session")
                .padding(.top, 6)
                Spacer()
            }
        }
    }

    private func save(_ pane: Pane) {
        let label = draft.trimmingCharacters(in: .whitespaces)
        guard !label.isEmpty, !saving else { return }
        if label == pane.sessionLabel {
            editing = false
            return
        }
        saving = true
        Task {
            error = await store.renameSession(of: pane, to: label)
            saving = false
            if error == nil { editing = false }
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
