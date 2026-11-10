import Foundation

// What the hub returns. Keys arrive in snake_case; see src/store.rs and
// src/methods.rs.

struct Alert: Decodable, Identifiable, Hashable {
    let id: Int
    let status: String
    let category: String  // needs_you | failed | finished
    let host: String
    let sessionId: String
    let blockId: String
    let recordId: String
    let state: String
    let kind: String?  // permission | question | auth
    let app: String?
    let title: String?
    let msg: String?
    let sessionLabel: String?
    let paneLabel: String?
    let cwd: String?
    let createdAt: String
    let resolvedReason: String?
    /// What the pane does for a tool that runs it, such as
    /// `api#12 · codex reviewer`. It stands in for the pane's place.
    let subject: String?
    /// What the item can be answered with, as the hub decides: the method
    /// to call and the button's label (Approve, or Post for a review).
    let answers: [Answer]?

    var place: String {
        if let subject, !subject.isEmpty { return host == "local" ? subject : "\(host):\(subject)" }
        return Format.place(host: host, session: sessionLabel, pane: paneLabel, block: blockId)
    }
    var appName: String { Format.appName(app) }

    /// One line saying what it is waiting on or did.
    var what: String {
        [title, msg].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ": ")
    }

    /// Whether it can be answered from here (the hub checks again).
    var canAnswer: Bool { !(answers ?? []).isEmpty }

    /// The answers that send something: approve and deny, without preview.
    var decisions: [Answer] { (answers ?? []).filter { $0.action != "preview" } }
    var canPreview: Bool { (answers ?? []).contains { $0.action == "preview" } }

    var headline: String {
        switch (category, kind) {
        case ("failed", _): return "\(appName) failed"
        case ("finished", _): return "\(appName) finished"
        case (_, "permission"): return "\(appName) needs your approval"
        case (_, "question"): return "\(appName) has a question"
        case (_, "auth"): return "\(appName) needs you to sign in"
        case (_, "startup"): return "\(appName) is waiting to start"
        default: return "\(appName) needs you"
        }
    }
}

/// One answer an item offers: `approve`, `deny`, or `preview`.
struct Answer: Decodable, Hashable {
    let action: String
    let label: String
}

struct Message: Decodable, Identifiable, Hashable {
    let id: Int
    let threadId: Int
    let replyTo: Int?
    let fromAddr: String
    let fromBlock: String?
    let toAddr: String
    let toBlock: String?
    let subject: String?
    let body: String
    let status: String
    let createdAt: String
    let error: String?
    /// An app the sender serves for you to look at, on the tailnet.
    let preview: Preview?

    var from: String { fromBlock == nil ? "you" : fromAddr }

    var firstLine: String {
        if let s = subject, !s.isEmpty { return s }
        return body.split(separator: "\n").first.map(String.init) ?? ""
    }
}

/// A message's preview (src/preview.rs): where the app is, the answers the
/// agent offers, and whether it's still served.
struct Preview: Decodable, Hashable {
    let url: String
    let port: Int
    let options: [String]?
    let state: String
    let stopped: String?
    /// What you answered, once you have.
    let answer: String?

    var live: Bool { state == "live" }
}

struct Inbox: Decodable {
    let alerts: [Alert]
    let messages: [Message]
}

struct Pane: Decodable, Identifiable, Hashable {
    let host: String
    let sessionId: String
    let sessionLabel: String?
    let windowLabel: String?
    let blockId: String
    let label: String
    let app: String?
    let state: String?
    let kind: String?
    let msg: String?
    let title: String?
    let since: String?
    let cwd: String?
    let process: String?
    let workingChildren: Int
    let subject: String?

    var id: String { host + "|" + blockId }
    var session: String { (sessionLabel?.isEmpty == false ? sessionLabel : nil) ?? Format.shortID(sessionId) }
    var name: String { label.isEmpty ? Format.shortID(blockId) : label }
    var isLocal: Bool { host == "local" }
    var address: String { isLocal ? "\(session)/\(name)" : "\(host):\(session)/\(name)" }
    /// What the Agents list groups by: the session, and its host when it's another machine.
    var group: String { isLocal ? session : "\(host) · \(session)" }
}

/// A Rex host the hub follows, as `hosts.list` has it: where a new agent can go.
struct RexHost: Decodable, Identifiable, Hashable {
    let host: String
    let connected: Bool
    let error: String?
    let home: String?
    let sessions: [String]
    /// Folders its panes are in, agents' first.
    let folders: [String]
    let agents: Int

    var id: String { host }
    var isLocal: Bool { host == "local" }
    var title: String { isLocal ? "This Mac" : host }
}

/// A folder's subfolders on a host (`dirs.list`).
struct DirListing: Decodable {
    let status: String
    let path: String
    let dirs: [String]
}

/// An agent `agents.start` started.
struct Started: Decodable {
    let host: String
    let name: String
    let address: String
    let cwd: String
    let blockId: String
}

struct HubEvent: Decodable {
    let type: String
    let alert: Alert?
    let message: Message?
}

enum Format {
    static func appName(_ app: String?) -> String {
        switch app {
        case "claude-code": return "Claude Code"
        case "codex": return "Codex"
        case "rex-reviewer": return "Rex Reviewer"
        case let a?: return a
        case nil: return "A program"
        }
    }

    static func shortID(_ id: String) -> String {
        String(id.split(whereSeparator: { $0 == "-" || $0 == ":" }).last?.suffix(6) ?? Substring(id))
    }

    static func place(host: String, session: String?, pane: String?, block: String) -> String {
        let s = session ?? ""
        let p = (pane?.isEmpty == false ? pane! : shortID(block))
        let base = s.isEmpty ? p : "\(s)/\(p)"
        return host == "local" ? base : "\(host):\(base)"
    }

    static func tilde(_ path: String?) -> String {
        guard let path else { return "" }
        let home = NSHomeDirectory()
        return path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }

    /// RFC 3339 with or without fractions (Rex sends microseconds).
    static func date(_ s: String?) -> Date? {
        guard var s else { return nil }
        if let dot = s.firstIndex(of: "."), let end = s[dot...].firstIndex(where: { $0 == "Z" || $0 == "+" || $0 == "-" }) {
            s.removeSubrange(dot..<end)
        }
        return iso.date(from: s)
    }

    private static let iso: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// `45s`, `3m`, `2h`, `4d`.
    static func age(_ s: String?, now: Date = Date()) -> String {
        guard let d = date(s) else { return "" }
        let secs = max(0, Int(now.timeIntervalSince(d)))
        switch secs {
        case ..<60: return "\(secs)s"
        case ..<3600: return "\(secs / 60)m"
        case ..<86400: return "\(secs / 3600)h"
        default: return "\(secs / 86400)d"
        }
    }
}
