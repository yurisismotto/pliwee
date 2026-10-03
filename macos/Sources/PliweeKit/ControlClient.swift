// The client half of the control socket.
//
// The same exchange `pliwee` (the CLI) and the GTK application make: connect
// to the agent's Unix socket, write one JSON request and a newline, read
// newline-delimited JSON back. Plain POSIX sockets, because Foundation has no
// Unix-domain stream API and Network.framework's would bring a second
// lifecycle model for what is a local, blocking, line-oriented exchange.
//
// Every blocking call runs off the main thread.

import Darwin
import Foundation

/// Why talking to the agent did not work.
public enum ControlError: Error, Equatable, LocalizedError {
    /// Nothing is listening at the socket, or it does not exist.
    case unreachable(path: String, reason: String)
    /// The socket path does not fit in `sockaddr_un`.
    case pathTooLong(path: String)
    /// The agent did not answer in time.
    case timedOut
    /// The agent closed the connection without answering.
    case closed
    /// A line that is not the protocol.
    case malformed(String)
    /// The agent answered with `{"error": ...}`.
    case agent(String)

    public var errorDescription: String? {
        switch self {
        case let .unreachable(path, reason):
            return "Could not reach the Pliwee service at \(path): \(reason)"
        case let .pathTooLong(path):
            return "The control socket path is too long for macOS: \(path)"
        case .timedOut: return "The Pliwee service did not answer in time."
        case .closed: return "The Pliwee service closed the connection."
        case let .malformed(why): return "Unexpected reply from the Pliwee service: \(why)"
        case let .agent(message): return message
        }
    }
}

/// One connection to the agent's socket.
public final class ControlConnection: @unchecked Sendable {
    private let fd: Int32
    private var buffer = Data()
    private let lock = NSLock()
    private var closed = false

    /// The longest line accepted from the agent. A status report is a few
    /// kilobytes; this bounds memory if something else answers on the socket.
    static let maxLineBytes = 8 * 1024 * 1024

    /// Connects, with `timeout` applied to every later read and write.
    /// `nil` waits indefinitely — for the streams, which last as long as a
    /// pairing window or the application.
    public init(path: String, timeout: TimeInterval?) throws {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw ControlError.unreachable(path: path, reason: String(cString: strerror(errno)))
        }
        // A write to an agent that has gone away must be an error, not a
        // SIGPIPE that ends the application.
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        if let timeout {
            var tv = timeval(tv_sec: Int(timeout), tv_usec: Int32((timeout.truncatingRemainder(dividingBy: 1)) * 1_000_000))
            setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
            setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        }
        do {
            try Self.connect(fd, to: path)
        } catch {
            Darwin.close(fd)
            throw error
        }
        self.fd = fd
    }

    /// `connect(2)` to a filesystem socket. The one place that touches
    /// `sockaddr_un` by pointer.
    private static func connect(_ fd: Int32, to path: String) throws {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard bytes.count < capacity else { throw ControlError.pathTooLong(path: path) }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
        }
        let length = socklen_t(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, length) }
        }
        guard result == 0 else {
            throw ControlError.unreachable(path: path, reason: String(cString: strerror(errno)))
        }
    }

    deinit {
        close()
        Darwin.close(fd)
    }

    /// Writes one request line.
    public func send(_ request: Request) throws {
        let line = try request.line()
        try line.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let n = Darwin.write(fd, raw.baseAddress! + offset, raw.count - offset)
                if n < 0 {
                    if errno == EINTR { continue }
                    if errno == EAGAIN || errno == EWOULDBLOCK { throw ControlError.timedOut }
                    throw ControlError.closed
                }
                offset += n
            }
        }
    }

    /// Reads one line, without its newline. `nil` at a clean end of stream.
    public func readLine() throws -> Data? {
        while true {
            if let newline = buffer.firstIndex(of: 0x0A) {
                let line = buffer[buffer.startIndex..<newline]
                buffer.removeSubrange(buffer.startIndex...newline)
                return Data(line)
            }
            guard buffer.count < Self.maxLineBytes else {
                throw ControlError.malformed("a line longer than \(Self.maxLineBytes) bytes")
            }
            var chunk = [UInt8](repeating: 0, count: 64 * 1024)
            let n = chunk.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
            if n > 0 {
                buffer.append(contentsOf: chunk[0..<n])
            } else if n == 0 {
                return buffer.isEmpty ? nil : { defer { buffer.removeAll() }; return buffer }()
            } else if errno == EINTR {
                continue
            } else if errno == EAGAIN || errno == EWOULDBLOCK {
                throw ControlError.timedOut
            } else {
                throw ControlError.closed
            }
        }
    }

    /// Ends the connection. A read blocked on another thread returns.
    ///
    /// `shutdown(2)` only: the descriptor itself is released in `deinit`,
    /// when no thread can still be reading it. Closing it here could hand
    /// its number to an unrelated file while a reader was about to use it.
    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return }
        closed = true
        Darwin.shutdown(fd, SHUT_RDWR)
    }
}

/// Requests and streams against one socket path.
public struct ControlClient: Sendable {
    public let socketPath: String

    public init(socketPath: String) {
        self.socketPath = socketPath
    }

    public init(paths: RuntimePaths = .forCurrentUser()) {
        self.init(socketPath: paths.controlSocket.path)
    }

    /// One request, one reply. `{"error": ...}` is thrown as `.agent`.
    public func request(_ request: Request, timeout: TimeInterval = 5) async throws -> Response {
        let path = socketPath
        return try await Task.detached(priority: .userInitiated) {
            let connection = try ControlConnection(path: path, timeout: timeout)
            defer { connection.close() }
            try connection.send(request)
            guard let line = try connection.readLine() else { throw ControlError.closed }
            let response: Response
            do {
                response = try ControlCoding.response(from: line)
            } catch {
                throw ControlError.malformed(String(describing: error))
            }
            if case let .error(message) = response { throw ControlError.agent(message) }
            return response
        }.value
    }

    /// A request whose connection stays open: `pair`, `send`,
    /// `watch_file_offers`. Lines arrive on `messages`; `connection` stays
    /// writable for the answer the stream asks for, and closing it ends the
    /// stream on both sides.
    public func stream(_ request: Request) throws -> ControlStream {
        let connection = try ControlConnection(path: socketPath, timeout: nil)
        try connection.send(request)
        return ControlStream(connection: connection)
    }
}

/// An open streaming exchange.
public final class ControlStream: @unchecked Sendable {
    public let connection: ControlConnection

    init(connection: ControlConnection) {
        self.connection = connection
    }

    /// Every line the agent sends, until it closes the connection or this
    /// side does. Reading happens on its own thread.
    public var messages: AsyncThrowingStream<StreamMessage, Error> {
        let connection = connection
        return AsyncThrowingStream { continuation in
            let thread = Thread {
                do {
                    while let line = try connection.readLine() {
                        guard !line.isEmpty else { continue }
                        do {
                            continuation.yield(try ControlCoding.streamMessage(from: line))
                        } catch {
                            throw ControlError.malformed(String(describing: error))
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            thread.name = "Pliwee control stream"
            continuation.onTermination = { _ in connection.close() }
            thread.start()
        }
    }

    public func send(_ request: Request) throws {
        try connection.send(request)
    }

    public func close() {
        connection.close()
    }
}
