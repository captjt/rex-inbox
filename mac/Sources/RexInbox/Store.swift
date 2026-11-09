import AppKit
import Foundation
import Observation
import ServiceManagement

/// The app's state: what the hub says, kept current by its event stream.
@Observable
@MainActor
final class Store {
    var connected = false
    var alerts: [Alert] = []
    var messages: [Message] = []
    var panes: [Pane] = []
    /// The hosts a new agent can go on, this Mac first.
    var hosts: [RexHost] = []
    /// The last thing that went wrong, shown until the next action works.
    var problem: String?
    /// A short note on what the last action did.
    var note: String?
    /// Items with an action in flight.
    var busy: Set<Int> = []
    /// The review the preview window shows.
    var previewed: Previewed?
    var openAtLogin = SMAppService.mainApp.status == .enabled
    /// Ticks once a minute so ages stay current.
    var now = Date()

    @ObservationIgnored let notifier = Notifier()
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var noteTask: Task<Void, Never>?

    var needsYou: [Alert] { alerts.filter { $0.category == "needs_you" } }
    var failed: [Alert] { alerts.filter { $0.category == "failed" } }
    var finished: [Alert] { alerts.filter { $0.category == "finished" } }
    var unread: [Message] { messages.filter { $0.status == "unread" } }
    /// What the menu bar counts: things waiting on you.
    var waiting: Int { needsYou.count + unread.count }

    /// "2 need you · 1 message · 6 agents"
    var summary: String {
        guard connected else { return "Not connected" }
        var parts: [String] = []
        if !needsYou.isEmpty { parts.append("\(needsYou.count) need\(needsYou.count == 1 ? "s" : "") you") }
        if !messages.isEmpty { parts.append("\(messages.count) message\(messages.count == 1 ? "" : "s")") }
        if !failed.isEmpty { parts.append("\(failed.count) failed") }
        if parts.isEmpty { parts.append("All clear") }
        parts.append("\(panes.count) agent\(panes.count == 1 ? "" : "s")")
        return parts.joined(separator: " · ")
    }

    init() {
        if !Snapshot.isActive {
            notifier.start(store: self)
        }
        Hub.watch(
            onState: { up in Task { @MainActor in self.setConnected(up) } },
            onEvent: { data in Task { @MainActor in self.handle(data) } }
        )
        Task {
            while true {
                try? await Task.sleep(for: .seconds(60))
                self.now = Date()
            }
        }
        Snapshot.runIfAsked(self)
    }

    private func setConnected(_ up: Bool) {
        guard up != connected else { return }
        connected = up
        if up {
            scheduleRefresh()
        }
    }

    private func handle(_ data: Data) {
        guard let event = try? Hub.decoder.decode(HubEvent.self, from: data) else { return }
        scheduleRefresh()
        notifier.consider(event)
    }

    /// Coalesce a burst of events into one fetch.
    func scheduleRefresh() {
        refreshTask?.cancel()
        refreshTask = Task {
            try? await Task.sleep(for: .milliseconds(120))
            if Task.isCancelled { return }
            await refresh()
        }
    }

    func refresh() async {
        do {
            let inbox = try await Hub.call("inbox", as: Inbox.self)
            let panes = try await Hub.call("panes.list", as: [Pane].self)
            alerts = inbox.alerts
            messages = inbox.messages
            self.panes = panes
            now = Date()
        } catch {
            if connected { problem = error.localizedDescription }
        }
    }

    func alert(_ id: Int) -> Alert? { alerts.first { $0.id == id } }

    // MARK: - New agents

    func loadHosts() async {
        do {
            hosts = try await Hub.call("hosts.list", as: [RexHost].self)
        } catch {
            problem = error.localizedDescription
        }
    }

    /// A folder's subfolders on a host. Throws with the hub's reason.
    func dirs(host: String, path: String) async throws -> DirListing {
        try await Hub.call("dirs.list", ["host": host, "path": path], as: DirListing.self)
    }

    /// Start an agent; nil and a note when it started, else what went wrong.
    func startAgent(host: String, program: String, cwd: String, prompt: String, name: String, session: String) async -> String? {
        var params: [String: Any] = ["host": host, "program": program, "cwd": cwd, "prompt": prompt]
        if !name.isEmpty { params["name"] = name }
        if !session.isEmpty { params["session"] = session }
        do {
            let s = try await Hub.call("agents.start", params, as: Started.self)
            let app = program == "codex" ? "Codex" : "Claude Code"
            flash("Started \(app) as \(s.address) in \(Format.tilde(s.cwd))")
            problem = nil
            await refresh()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    // MARK: - Actions

    func approve(_ id: Int, seen: String?) async {
        await answer("approve", id, seen: seen)
    }

    func deny(_ id: Int, seen: String?) async {
        await answer("deny", id, seen: seen)
    }

    /// Approve or deny, as the item's answers name them. A review posts or
    /// is dismissed, and the hub says which in its summary.
    func answer(_ method: String, _ id: Int, seen: String?) async {
        var params: [String: Any] = ["id": id]
        if let seen { params["seen"] = seen }
        await act(id) {
            let data = try await Hub.call(method, params)
            let result = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            if let summary = result?["summary"] as? String {
                self.flash("#\(id): \(summary)")
            } else {
                let now = result?["now"] as? String ?? "?"
                self.flash("#\(id) \(method == "approve" ? "approved" : "denied"); the pane is now \(now)")
            }
        }
    }

    /// Fetch a review as it would post, for the preview window. False when
    /// it couldn't be had (the problem says why).
    func preview(_ a: Alert) async -> Bool {
        var ok = false
        await act(a.id) {
            let data = try await Hub.call("preview", ["id": a.id])
            let result = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            self.previewed = Previewed(alert: a, text: result?["text"] as? String ?? "")
            ok = true
        }
        return ok
    }

    /// Bring an item's pane to the front: the pane in Rex, then Rex itself.
    func open(_ id: Int) async {
        await act(id) {
            try await Hub.call("open", ["id": id])
            Self.activateRex()
        }
    }

    func open(_ pane: Pane) async {
        do {
            try await Hub.call("panes.open", ["host": pane.host, "block_id": pane.blockId])
            Self.activateRex()
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    func done(_ id: Int) async {
        await act(id) { try await Hub.call("done", ["id": id]) }
    }

    func snooze(_ id: Int, _ duration: String = "1h") async {
        await act(id) { try await Hub.call("alerts.snooze", ["id": id, "for": duration]) }
    }

    func reply(_ id: Int, _ text: String) async {
        await act(id) {
            try await Hub.call("messages.reply", ["id": id, "body": text, "caller": ["as_me": true]])
            self.flash("Reply to #\(id) queued; it goes in when the agent is idle")
        }
    }

    func send(to pane: Pane, _ text: String) async {
        do {
            try await Hub.call("messages.send", ["to": pane.blockId, "body": text, "caller": ["as_me": true]])
            flash("Sent to \(pane.address); it goes in when the agent is idle")
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    private func act(_ id: Int, _ work: () async throws -> Void) async {
        busy.insert(id)
        defer { busy.remove(id) }
        do {
            try await work()
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
        await refresh()
    }

    private func flash(_ text: String) {
        note = text
        noteTask?.cancel()
        noteTask = Task {
            try? await Task.sleep(for: .seconds(6))
            if !Task.isCancelled { note = nil }
        }
    }

    // MARK: - App

    static func activateRex() {
        NSRunningApplication.runningApplications(withBundleIdentifier: "com.superlogical.rex").first?.activate()
    }

    /// `rex-inbox up`: install and start the hub's LaunchAgent.
    func startHub() async {
        guard let exe = Self.cli() else {
            problem = "rex-inbox isn't installed; run cargo install --path . in the rex-inbox repo"
            return
        }
        let result: (Int32, String) = await withCheckedContinuation { cont in
            DispatchQueue.global().async {
                let p = Process()
                p.executableURL = URL(fileURLWithPath: exe)
                p.arguments = ["up"]
                let pipe = Pipe()
                p.standardOutput = pipe
                p.standardError = pipe
                do {
                    try p.run()
                    p.waitUntilExit()
                    let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                    cont.resume(returning: (p.terminationStatus, out))
                } catch {
                    cont.resume(returning: (-1, error.localizedDescription))
                }
            }
        }
        if result.0 == 0 {
            problem = nil
            flash(result.1.trimmingCharacters(in: .whitespacesAndNewlines))
        } else {
            problem = result.1.trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }

    static func cli() -> String? {
        let candidates = [NSHomeDirectory() + "/.cargo/bin/rex-inbox", "/opt/homebrew/bin/rex-inbox", "/usr/local/bin/rex-inbox"]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    func toggleOpenAtLogin() {
        do {
            if SMAppService.mainApp.status == .enabled {
                try SMAppService.mainApp.unregister()
            } else {
                try SMAppService.mainApp.register()
            }
        } catch {
            problem = "Open at Login: \(error.localizedDescription)"
        }
        openAtLogin = SMAppService.mainApp.status == .enabled
    }
}
