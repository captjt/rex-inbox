import AppKit
import SwiftUI

/// Start Claude Code or Codex on any Rex host the hub follows: pick the
/// machine, the folder there, and what to do. It opens in a new tab of the
/// host's `inbox` session (or the session named under More), named so it
/// can take messages, and shows up under Agents like any other.
struct NewAgentWindow: View {
    let store: Store
    @Environment(\.dismissWindow) private var dismissWindow

    @State private var host = "local"
    @State private var program = "claude"
    @State private var folder = ""
    @State private var prompt = ""
    @State private var name = ""
    @State private var session = ""
    @State private var starting = false
    @State private var error: String?
    @State private var browsing = false
    @State private var listing: DirListing?
    @State private var browseError: String?

    private var chosen: RexHost? { store.hosts.first { $0.host == host } }
    private var canStart: Bool { !starting && chosen?.connected == true }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Picker("Machine", selection: $host) {
                    ForEach(store.hosts) { h in
                        Text(h.connected ? h.title : "\(h.title) (unreachable)").tag(h.host)
                    }
                }
                if let h = chosen, !h.connected {
                    Text(h.error ?? "Rex on \(h.title) can't be reached.")
                        .font(.callout).foregroundStyle(.red)
                }
                Picker("Agent", selection: $program) {
                    Text("Claude Code").tag("claude")
                    Text("Codex").tag("codex")
                }
                .pickerStyle(.segmented)
                folderRow
                if browsing { browser }
                Section("Prompt") {
                    TextEditor(text: $prompt)
                        .font(.body)
                        .frame(minHeight: 120)
                        .scrollContentBackground(.hidden)
                        .padding(6)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .textBackgroundColor)))
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                        .labelsHidden()
                }
                Section {
                    DisclosureGroup("More") {
                        TextField("Name", text: $name, prompt: Text(program))
                        HStack {
                            TextField("Session", text: $session, prompt: Text("inbox"))
                            if let h = chosen, !h.sessions.isEmpty {
                                Menu {
                                    ForEach(h.sessions, id: \.self) { s in Button(s) { session = s } }
                                } label: {
                                    Image(systemName: "chevron.up.chevron.down")
                                }
                                .menuStyle(.borderlessButton)
                                .fixedSize()
                            }
                        }
                        Text("It opens in a new tab of this session, made if it doesn't exist. Other agents message it by its name.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)
            Divider()
            HStack(spacing: 8) {
                if let error {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
                    Text(error).font(.callout).foregroundStyle(.red).lineLimit(3).textSelection(.enabled)
                } else if prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Text("Without a prompt, the agent waits for you in its pane.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                if starting { ProgressView().controlSize(.small) }
                Button("Cancel") { dismissWindow(id: "new-agent") }
                    .keyboardShortcut(.cancelAction)
                Button("Start") { start() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(!canStart)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
        .frame(minWidth: 560, idealWidth: 600, minHeight: 520)
        .task { await store.loadHosts() }
        .onChange(of: host) {
            folder = ""
            session = ""
            listing = nil
            browsing = false
            error = nil
        }
        .onAppear { NSApp.setActivationPolicy(.regular) }
        .onDisappear { NSApp.setActivationPolicy(.accessory) }
    }

    private var folderRow: some View {
        LabeledContent("Folder") {
            HStack(spacing: 6) {
                TextField("Folder", text: $folder, prompt: Text(chosen?.isLocal == false ? "~ on \(host)" : "~"))
                    .labelsHidden()
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: .infinity)
                if let h = chosen, !h.folders.isEmpty {
                    Menu {
                        Section("Where agents are") {
                            ForEach(h.folders, id: \.self) { f in Button(tilde(f, h)) { folder = f } }
                        }
                    } label: {
                        Image(systemName: "clock.arrow.circlepath")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .help("Folders this machine's agents are in")
                }
                Button {
                    browsing.toggle()
                    if browsing { Task { await browse(folder.isEmpty ? "~" : folder) } }
                } label: {
                    Image(systemName: "folder")
                }
                .buttonStyle(.borderless)
                .help("Browse folders on \(chosen?.title ?? "this machine")")
                .disabled(chosen?.connected != true)
            }
        }
    }

    /// Folders on the chosen host, one level at a time.
    private var browser: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Button {
                    if let p = listing?.path { Task { await browse(parent(p)) } }
                } label: {
                    Image(systemName: "arrow.up")
                }
                .buttonStyle(.borderless)
                .disabled(listing == nil || listing?.path == "/")
                Text(listing.map { tilde($0.path, chosen) } ?? "…")
                    .font(.callout.monospaced()).lineLimit(1).truncationMode(.head)
                Spacer()
            }
            if let browseError {
                Text(browseError).font(.callout).foregroundStyle(.red)
            } else if let l = listing {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(l.dirs, id: \.self) { d in
                            Button {
                                Task { await browse(join(l.path, d)) }
                            } label: {
                                Label(d, systemImage: "folder").frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.vertical, 3).contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                        if l.dirs.isEmpty {
                            Text("No folders in here.").font(.callout).foregroundStyle(.secondary)
                        }
                    }
                }
                .frame(height: 150)
            }
        }
    }

    private func browse(_ path: String) async {
        do {
            let l = try await store.dirs(host: host, path: path)
            if l.status != "ok" {
                browseError = "\(tilde(l.path, chosen)) isn't a folder there."
                return
            }
            listing = l
            folder = l.path
            browseError = nil
        } catch {
            browseError = error.localizedDescription
        }
    }

    private func start() {
        starting = true
        error = nil
        Task {
            let failed = await store.startAgent(
                host: host,
                program: program,
                cwd: folder.trimmingCharacters(in: .whitespaces),
                prompt: prompt,
                name: name.trimmingCharacters(in: .whitespaces),
                session: session.trimmingCharacters(in: .whitespaces)
            )
            starting = false
            if let failed {
                error = failed
            } else {
                prompt = ""
                name = ""
                dismissWindow(id: "new-agent")
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
