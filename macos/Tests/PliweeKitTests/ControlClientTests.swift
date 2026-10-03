// The client against a real Unix socket.
//
// `FakeAgent` binds a socket in a temporary directory and answers each
// connection from a script, so these tests exercise the actual `connect`,
// `write` and line reading the app uses — not a mock of them.

import Darwin
import Foundation
import Testing
@testable import PliweeKit

final class FakeAgent: @unchecked Sendable {
    let path: String
    private let fd: Int32
    private let lock = NSLock()
    private var _received: [String] = []

    var received: [String] {
        lock.lock(); defer { lock.unlock() }
        return _received
    }

    /// `script` is called with each connection's first line and returns the
    /// lines to answer with; `holdOpen` keeps the connection open afterwards
    /// and records every further line, as a stream does.
    init(holdOpen: Bool = false, script: @escaping @Sendable (String) -> [String]) throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("pliwee-test-\(UUID().uuidString.prefix(8))")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        path = dir.appendingPathComponent("c.sock").path
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: bytes); raw[bytes.count] = 0
        }
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, listen(fd, 8) == 0 else { throw ControlError.closed }
        let listener = fd
        let thread = Thread { [weak self] in
            while true {
                let client = accept(listener, nil, nil)
                if client < 0 { return }
                Thread {
                    self?.serve(client, holdOpen: holdOpen, script: script)
                }.start()
            }
        }
        thread.start()
    }

    private func serve(_ client: Int32, holdOpen: Bool, script: (String) -> [String]) {
        defer { Darwin.close(client) }
        var buffer = Data()
        func readLine() -> String? {
            while true {
                if let nl = buffer.firstIndex(of: 0x0A) {
                    let line = String(decoding: buffer[buffer.startIndex..<nl], as: UTF8.self)
                    buffer.removeSubrange(buffer.startIndex...nl)
                    return line
                }
                var chunk = [UInt8](repeating: 0, count: 4096)
                let n = read(client, &chunk, chunk.count)
                if n <= 0 { return nil }
                buffer.append(contentsOf: chunk[0..<n])
            }
        }
        guard let first = readLine() else { return }
        record(first)
        for reply in script(first) {
            let bytes = Array((reply + "\n").utf8)
            _ = bytes.withUnsafeBytes { write(client, $0.baseAddress, $0.count) }
        }
        if holdOpen {
            while let line = readLine() { record(line) }
        }
    }

    private func record(_ line: String) {
        lock.lock(); _received.append(line); lock.unlock()
    }

    deinit {
        Darwin.close(fd)
        unlink(path)
    }
}

@Suite struct ControlClientTests {
    @Test func aRequestIsOneJsonLineAndTheReplyIsDecoded() async throws {
        let agent = try FakeAgent { _ in [#"{"pong":{"rtt_ms":7}}"#] }
        let client = ControlClient(socketPath: agent.path)
        let reply = try await client.request(.ping(device: "abcd"))
        #expect(reply == .pong(rttMs: 7))
        let sent = try #require(agent.received.first)
        let object = try #require(try JSONSerialization.jsonObject(with: Data(sent.utf8)) as? [String: String])
        #expect(object == ["cmd": "ping", "device": "abcd"])
    }

    @Test func anAgentErrorIsThrownWithItsMessage() async throws {
        let agent = try FakeAgent { _ in [#"{"error":{"message":"no such device"}}"#] }
        await #expect(throws: ControlError.agent("no such device")) {
            _ = try await ControlClient(socketPath: agent.path).request(.unpair(device: "x"))
        }
    }

    @Test func aConnectionClosedWithoutAReplyIsAnError() async throws {
        let agent = try FakeAgent { _ in [] }
        await #expect(throws: ControlError.closed) {
            _ = try await ControlClient(socketPath: agent.path).request(.status)
        }
    }

    @Test func aMissingSocketIsUnreachableAndNamesThePath() async throws {
        let path = FileManager.default.temporaryDirectory.appendingPathComponent("nothing-\(UUID()).sock").path
        do {
            _ = try await ControlClient(socketPath: path).request(.status)
            Issue.record("expected unreachable")
        } catch let ControlError.unreachable(reported, _) {
            #expect(reported == path)
        }
    }

    @Test func aSocketPathThatCannotFitIsRefusedByName() async throws {
        let path = "/" + String(repeating: "p", count: 200)
        await #expect(throws: ControlError.pathTooLong(path: path)) {
            _ = try await ControlClient(socketPath: path).request(.status)
        }
    }

    @Test func aGarbageReplyIsMalformedNotACrash() async throws {
        let agent = try FakeAgent { _ in ["this is not json"] }
        do {
            _ = try await ControlClient(socketPath: agent.path).request(.status)
            Issue.record("expected malformed")
        } catch ControlError.malformed {
        }
    }

    @Test func aStreamDeliversEventsAndCarriesTheAnswerBackOnTheSameConnection() async throws {
        let agent = try FakeAgent(holdOpen: true) { _ in [
            #"{"event":"pairing_ready","payload":"p","qr_ascii":"q","expires_in_secs":60}"#,
            #"{"event":"confirm_request","device_name":"Phone","device_id":"d","fingerprint":"ff","fingerprint_short":"FF"}"#,
        ] }
        let stream = try ControlClient(socketPath: agent.path).stream(.pair(ttlSecs: 60))
        var seen: [StreamMessage] = []
        for try await message in stream.messages {
            seen.append(message)
            if case .event(.confirmRequest) = message {
                try stream.send(.confirm(accept: false))
                break
            }
        }
        #expect(seen.first == .event(.pairingReady(payload: "p", qrAscii: "q", expiresInSecs: 60)))
        // The answer reached the agent on the connection the question came on.
        for _ in 0..<50 where agent.received.count < 2 { try await Task.sleep(nanoseconds: 20_000_000) }
        #expect(agent.received.count == 2)
        #expect(agent.received.last.map { $0.contains(#""cmd":"confirm""#) && $0.contains(#""accept":false"#) } == true)
        stream.close()
    }

    @Test func closingAStreamEndsItsMessages() async throws {
        let agent = try FakeAgent(holdOpen: true) { _ in [#"{"event":"file_approval_ready","unattended":false}"#] }
        let stream = try ControlClient(socketPath: agent.path).stream(.watchFileOffers)
        var count = 0
        let task = Task {
            for try await _ in stream.messages { count += 1 }
            return count
        }
        try await Task.sleep(nanoseconds: 200_000_000)
        stream.close()
        let total = try await task.value
        #expect(total == 1)
    }
}
