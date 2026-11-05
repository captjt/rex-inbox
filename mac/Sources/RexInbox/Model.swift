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

    var place: String { Format.place(host: host, session: sessionLabel, pane: paneLabel, block: blockId) }
    var appName: String { Format.appName(app) }

    /// One line saying what it is waiting on or did.
    var what: String {
        [title, msg].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: ": ")
    }

    /// Whether the app can answer this prompt with a key (the hub checks again).
    var canAnswer: Bool { category == "needs_you" && kind == "permission" && app == "claude-code" }

    var headline: String {
        switch (category, kind) {
        case ("failed", _): return "\(appName) failed"
        case ("finished", _): return "\(appName) finished"
        case (_, "permission"): return "\(appName) needs your approval"
        case (_, "question"): return "\(appName) has a question"
        case (_, "auth"): return "\(appName) needs you to sign in"
        default: return "\(appName) needs you"
        }
    }
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

    var from: String { fromBlock == nil ? "you" : fromAddr }

    var firstLine: String {
        if let s = subject, !s.isEmpty { return s }
        return body.split(separator: "\n").first.map(String.init) ?? ""
    }
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

    var id: String { host + "|" + blockId }
    var session: String { (sessionLabel?.isEmpty == false ? sessionLabel : nil) ?? Format.shortID(sessionId) }
    var name: String { label.isEmpty ? Format.shortID(blockId) : label }
    var address: String { "\(session)/\(name)" }
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
