import Foundation

// The hub's unix socket: one JSON line in, one JSON line out. `watch` keeps
// the connection open and writes one event per line. See src/api.rs.

enum HubError: LocalizedError {
    case notRunning
    case closed
    case hub(String)

    var errorDescription: String? {
        switch self {
        case .notRunning: return "The rex-inbox hub isn't running."
        case .closed: return "The hub closed the connection."
        case .hub(let message): return message
        }
    }
}

enum Hub {
    static var socketPath: String {
        let env = ProcessInfo.processInfo.environment
        if let path = env["REX_INBOX_SOCKET"] { return path }
        let state = env["REX_INBOX_STATE"] ?? (NSHomeDirectory() + "/.local/state/rex-inbox")
        return state + "/hub.sock"
    }

    static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()

    /// Call a method and decode its result.
    static func call<T: Decodable>(_ method: String, _ params: [String: Any] = [:], as: T.Type) async throws -> T {
        try decoder.decode(T.self, from: try await call(method, params))
    }

    /// Call a method; returns its result as JSON.
    @discardableResult
    static func call(_ method: String, _ params: [String: Any] = [:]) async throws -> Data {
        try await withCheckedThrowingContinuation { cont in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    cont.resume(returning: try callSync(method, params))
                } catch {
                    cont.resume(throwing: error)
                }
            }
        }
    }

    static func callSync(_ method: String, _ params: [String: Any]) throws -> Data {
        let fd = try connect()
        defer { close(fd) }
        try send(fd, method: method, params: params)
        guard let line = Lines(fd).next() else { throw HubError.closed }
        return try unwrap(line)
    }

    /// Follow the hub's events on a thread of their own, reconnecting for as
    /// long as the app runs. `notifier` tells the hub this app posts the
    /// notifications, so the hub's own stay quiet.
    static func watch(onState: @escaping (Bool) -> Void, onEvent: @escaping (Data) -> Void) {
        let thread = Thread {
            while true {
                if let fd = try? connect() {
                    if (try? send(fd, method: "watch", params: ["notifier": true])) != nil {
                        let lines = Lines(fd)
                        if let first = lines.next(), (try? unwrap(first)) != nil {
                            onState(true)
                            while let line = lines.next() {
                                onEvent(line)
                            }
                        }
                    }
                    close(fd)
                }
                onState(false)
                Thread.sleep(forTimeInterval: 2)
            }
        }
        thread.name = "rex-inbox watch"
        thread.start()
    }

    // MARK: - Socket

    private static func connect() throws -> Int32 {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        if fd < 0 { throw HubError.notRunning }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let path = Array(socketPath.utf8CString)
        guard path.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
            close(fd)
            throw HubError.notRunning
        }
        withUnsafeMutableBytes(of: &addr.sun_path) { dest in
            path.withUnsafeBytes { dest.copyMemory(from: $0) }
        }
        let rc = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if rc != 0 {
            close(fd)
            throw HubError.notRunning
        }
        return fd
    }

    private static func send(_ fd: Int32, method: String, params: [String: Any]) throws {
        var data = try JSONSerialization.data(withJSONObject: ["method": method, "params": params])
        data.append(0x0A)
        try data.withUnsafeBytes { buf in
            var off = 0
            while off < buf.count {
                let n = Darwin.write(fd, buf.baseAddress! + off, buf.count - off)
                if n <= 0 { throw HubError.closed }
                off += n
            }
        }
    }

    private static func unwrap(_ line: Data) throws -> Data {
        guard let obj = try JSONSerialization.jsonObject(with: line) as? [String: Any] else {
            throw HubError.closed
        }
        if obj["ok"] as? Bool == true {
            return try JSONSerialization.data(withJSONObject: obj["result"] ?? NSNull(), options: [.fragmentsAllowed])
        }
        throw HubError.hub(obj["error"] as? String ?? "unknown error")
    }

    private final class Lines {
        let fd: Int32
        var buffer = Data()

        init(_ fd: Int32) { self.fd = fd }

        func next() -> Data? {
            while true {
                if let i = buffer.firstIndex(of: 0x0A) {
                    let line = Data(buffer[buffer.startIndex..<i])
                    buffer.removeSubrange(buffer.startIndex...i)
                    return line
                }
                var chunk = [UInt8](repeating: 0, count: 65536)
                let n = read(fd, &chunk, chunk.count)
                if n <= 0 { return nil }
                buffer.append(contentsOf: chunk[0..<n])
            }
        }
    }
}
