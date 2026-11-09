import AppKit
import SwiftUI

/// What the New Agent form holds. It lives in the store, so closing the
/// panel halfway through doesn't lose it.
struct AgentDraft {
    var host = "local"
    var program = "claude"
    var folder = ""
    var prompt = ""
    var name = ""
    var session = ""
}

/// Start Claude Code or Codex on any Rex host the hub follows: pick the
/// machine, the folder there, and what to do. It opens in a new tab of the
/// host's `inbox` session (or the session named), named so it can take
/// messages, and shows up under Agents like any other.
///
/// The same form as the phone's New Agent sheet. It takes the menu bar
/// panel's place, and is a sheet over the full window.
struct NewAgentForm: View {
    @Bindable var store: Store
    /// Back to where it was opened from; `true` when an agent started.
    let close: (_ started: Bool) -> Void

    @State private var starting = false
    @State private var error: String?
    @State private var browsing = false
    @State private var listing: DirListing?
    @State private var browseError: String?
    @FocusState private var promptFocused: Bool

    private var draft: AgentDraft { store.draft }
    private var chosen: RexHost? { store.hosts.first { $0.host == draft.host } }
    private var canStart: Bool { !starting && chosen?.connected == true }
    private var programTitle: String { draft.program == "codex" ? "Codex" : "Claude Code" }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            VStack(alignment: .leading, spacing: 0) {
                machine
                agent
                folder
                prompt
                nameAndSession
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 16)
            Divider()
            footer
        }
        .task {
            await store.loadHosts()
            if chosen == nil, let first = store.hosts.first { store.draft.host = first.host }
        }
        .onAppear {
            promptFocused = true
            if Snapshot.isActive, CommandLine.arguments.contains("--browse") {
                browsing = true
                Task { await browse("~") }
            }
        }
    }

    // MARK: - Parts

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: "plus")
                .font(.title3.weight(.semibold))
                .foregroundStyle(Color.accentColor)
                .frame(width: 34, height: 34)
                .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(Color.accentColor.opacity(0.15)))
            VStack(alignment: .leading, spacing: 1) {
                Text("New Agent").font(.title3.weight(.semibold))
                Text("Claude Code or Codex, on any Rex host").font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            Button { close(false) } label: {
                Image(systemName: "xmark")
                    .font(.body.weight(.semibold))
                    .frame(width: 28, height: 28)
                    .background(Circle().fill(Color.primary.opacity(0.08)))
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .keyboardShortcut(.cancelAction)
            .help("Close (Esc)")
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 14)
    }

    private var machine: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel("Machine")
            if store.hosts.isEmpty {
                Text("Loading…").font(.callout).foregroundStyle(.secondary)
            } else {
                Flow(spacing: 8) {
                    ForEach(store.hosts) { h in
                        Chip(h.connected ? h.title : "\(h.title) · unreachable", on: h.host == draft.host, dim: !h.connected) {
                            choose(host: h.host)
                        }
                    }
                }
            }
            if let h = chosen, !h.connected {
                Note(h.error ?? "Rex on \(h.title) can't be reached.", bad: true)
            }
        }
    }

    private var agent: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel("Agent")
            HStack(spacing: 8) {
                Chip("Claude Code", on: draft.program == "claude") { store.draft.program = "claude" }
                Chip("Codex", on: draft.program == "codex") { store.draft.program = "codex" }
            }
        }
    }

    private var folder: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                FieldLabel("Folder")
                Spacer()
                Button {
                    browsing.toggle()
                    if browsing { Task { await browse(draft.folder.isEmpty ? "~" : draft.folder) } }
                } label: {
                    Label(browsing ? "Hide" : "Browse", systemImage: "folder")
                }
                .buttonStyle(.borderless)
                .disabled(chosen?.connected != true)
                .padding(.top, 14)
            }
            Field(text: $store.draft.folder, hint: chosen?.isLocal == false ? "~ on \(chosen?.title ?? "")" : "~", mono: true)
                .fieldBox()
            if browsing {
                browser
            } else if let h = chosen, !h.folders.isEmpty {
                Flow(spacing: 6) {
                    ForEach(h.folders.prefix(6), id: \.self) { f in
                        Chip(tilde(f, h), on: f == draft.folder, small: true) { store.draft.folder = f }
                    }
                }
            }
        }
    }

    /// Folders on the chosen host, one level at a time.
    @ViewBuilder private var browser: some View {
        if let browseError {
            Note(browseError, bad: true)
        } else if let l = listing {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    BrowseRow(symbol: "arrow.up", title: tilde(l.path, chosen), mono: true) {
                        Task { await browse(parent(l.path)) }
                    }
                    .disabled(l.path == "/")
                    ForEach(l.dirs, id: \.self) { d in
                        Divider().padding(.leading, 38)
                        BrowseRow(symbol: "folder", title: d) { Task { await browse(join(l.path, d)) } }
                    }
                    if l.dirs.isEmpty {
                        Divider().padding(.leading, 38)
                        Text("No folders in here.").font(.callout).foregroundStyle(.secondary)
                            .padding(.horizontal, 12).padding(.vertical, 7)
                    }
                }
            }
            .frame(maxHeight: 190)
            .fixedSize(horizontal: false, vertical: true)
            .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.primary.opacity(0.045)))
        } else {
            Note("Loading…")
        }
    }

    private var prompt: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel("Prompt")
            ZStack(alignment: .topLeading) {
                TextEditor(text: $store.draft.prompt)
                    .font(.body)
                    .scrollContentBackground(.hidden)
                    .focused($promptFocused)
                    .frame(height: 110)
                if draft.prompt.isEmpty {
                    Text("What should it do? Leave empty and it waits for you.")
                        .foregroundStyle(.tertiary)
                        .padding(.leading, 5)
                        .allowsHitTesting(false)
                }
            }
            .fieldBox()
        }
    }

    private var nameAndSession: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel("Name and session")
            HStack(spacing: 8) {
                Field(text: $store.draft.name, hint: draft.program)
                    .fieldBox()
                HStack(spacing: 4) {
                    Field(text: $store.draft.session, hint: "inbox")
                    if let h = chosen, !h.sessions.isEmpty {
                        Menu {
                            ForEach(h.sessions, id: \.self) { s in Button(s) { store.draft.session = s } }
                        } label: {
                            Image(systemName: "chevron.up.chevron.down")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .fixedSize()
                        .help("Sessions on \(h.title)")
                    }
                }
                .fieldBox()
            }
            Note("Other agents message it by this name. The session is made if it doesn't exist.")
        }
    }

    private var footer: some View {
        HStack(spacing: 10) {
            if let error {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
                Text(error).font(.callout).foregroundStyle(.red).lineLimit(3).textSelection(.enabled)
            } else {
                Text("A new tab in \(draft.session.isEmpty ? "inbox" : draft.session) on \(chosen?.title ?? "this Mac")")
                    .font(.callout).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Button(action: start) {
                HStack(spacing: 6) {
                    if starting {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "arrow.up")
                    }
                    Text("Start \(programTitle)")
                }
                .padding(.horizontal, 6)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!canStart)
            .help("Start (⌘↩)")
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    // MARK: - Actions

    private func choose(host: String) {
        guard host != draft.host else { return }
        store.draft.host = host
        store.draft.folder = ""
        store.draft.session = ""
        listing = nil
        browseError = nil
        error = nil
        if browsing { Task { await browse("~") } }
    }

    private func browse(_ path: String) async {
        do {
            let l = try await store.dirs(host: draft.host, path: path)
            if l.status != "ok" {
                browseError = "\(tilde(l.path, chosen)) isn't a folder there."
                return
            }
            listing = l
            store.draft.folder = tilde(l.path, chosen)
            browseError = nil
        } catch {
            browseError = error.localizedDescription
        }
    }

    private func start() {
        guard canStart else { return }
        starting = true
        error = nil
        let d = draft
        Task {
            let failed = await store.startAgent(
                host: d.host,
                program: d.program,
                cwd: d.folder.trimmingCharacters(in: .whitespaces),
                prompt: d.prompt,
                name: d.name.trimmingCharacters(in: .whitespaces),
                session: d.session.trimmingCharacters(in: .whitespaces)
            )
            starting = false
            if let failed {
                error = failed
            } else {
                // Keep the machine, agent and folder for the next one.
                store.draft.prompt = ""
                store.draft.name = ""
                browsing = false
                close(true)
            }
        }
    }

    private func tilde(_ path: String, _ h: RexHost?) -> String {
        guard let home = h?.home ?? (h?.isLocal == true ? NSHomeDirectory() : nil), path.hasPrefix(home) else { return path }
        return "~" + path.dropFirst(home.count)
    }

    private func parent(_ path: String) -> String {
        let p = (path as NSString).deletingLastPathComponent
        return p.isEmpty ? "/" : p
    }

    private func join(_ dir: String, _ name: String) -> String {
        (dir as NSString).appendingPathComponent(name)
    }
}

// MARK: - Pieces

private struct FieldLabel: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
            .padding(.top, 14)
    }
}

private struct Note: View {
    let text: String
    var bad = false
    init(_ text: String, bad: Bool = false) {
        self.text = text
        self.bad = bad
    }

    var body: some View {
        Text(text).font(.callout).foregroundStyle(bad ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A text field whose placeholder is plainly not a value.
private struct Field: View {
    @Binding var text: String
    let hint: String
    var mono = false

    var body: some View {
        ZStack(alignment: .leading) {
            if text.isEmpty {
                Text(hint).foregroundStyle(.tertiary).allowsHitTesting(false)
            }
            TextField("", text: $text).textFieldStyle(.plain)
        }
        .font(mono ? .body.monospaced() : .body)
    }
}

/// A choice, as the phone shows them: filled when chosen.
private struct Chip: View {
    let title: String
    let on: Bool
    var dim = false
    var small = false
    let action: () -> Void
    @State private var hover = false

    init(_ title: String, on: Bool, dim: Bool = false, small: Bool = false, action: @escaping () -> Void) {
        self.title = title
        self.on = on
        self.dim = dim
        self.small = small
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(small ? .callout.monospaced() : .body.weight(.medium))
                .lineLimit(1)
                .padding(.horizontal, small ? 10 : 14)
                .padding(.vertical, small ? 4 : 6)
                .foregroundStyle(on ? AnyShapeStyle(Color(nsColor: .windowBackgroundColor)) : dim ? AnyShapeStyle(.secondary) : AnyShapeStyle(.primary))
                .background(Capsule().fill(on ? Color.primary : Color.primary.opacity(hover ? 0.12 : 0.07)))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}

private struct BrowseRow: View {
    let symbol: String
    let title: String
    var mono = false
    let action: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: symbol).foregroundStyle(Color.accentColor).frame(width: 16)
                Text(title).font(mono ? .callout.monospaced() : .body).lineLimit(1).truncationMode(.head)
                Spacer()
                if symbol == "folder" {
                    Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(hover ? Color.primary.opacity(0.05) : .clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}

private extension View {
    /// A text field's box, as the phone draws them.
    func fieldBox() -> some View {
        padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(nsColor: .textBackgroundColor)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Color.primary.opacity(0.12)))
    }
}

/// Lays out its views in rows, wrapping when a row is full.
private struct Flow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = rows(subviews, width: proposal.width ?? .infinity)
        let height = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(rows.count - 1, 0))
        let width = rows.map(\.width).max() ?? 0
        return CGSize(width: proposal.width ?? width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in rows(subviews, width: bounds.width) {
            var x = bounds.minX
            for i in row.items {
                let size = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func rows(_ subviews: Subviews, width: CGFloat) -> [Row] {
        var rows: [Row] = [Row()]
        for i in subviews.indices {
            let size = subviews[i].sizeThatFits(.unspecified)
            let extra = rows[rows.count - 1].items.isEmpty ? size.width : size.width + spacing
            if rows[rows.count - 1].width + extra > width, !rows[rows.count - 1].items.isEmpty {
                rows.append(Row())
            }
            let last = rows.count - 1
            rows[last].width += rows[last].items.isEmpty ? size.width : size.width + spacing
            rows[last].height = max(rows[last].height, size.height)
            rows[last].items.append(i)
        }
        return rows
    }
}
