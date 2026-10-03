// Where the macOS agent keeps things.
//
// The Swift twin of `desktop/platform-macos/src/paths.rs`, and it must stay
// one: the agent binds its socket where that file says, and this app connects
// where this file says. Both derive every path from the home directory alone,
// and `RuntimePathsTests` asserts the same literal paths the Rust tests do.

import Foundation

public struct RuntimePaths: Equatable, Sendable {
    /// The directory name under `Application Support` and `Logs`.
    public static let appDirectory = "Pliwee"

    public let home: URL

    public init(home: URL) {
        self.home = home
    }

    /// Paths for the user running this process. `$HOME` when it is set to an
    /// absolute path, as `paths.rs` does; the account database otherwise.
    public static func forCurrentUser(environment: [String: String] = ProcessInfo.processInfo.environment) -> RuntimePaths {
        if let home = environment["HOME"], home.hasPrefix("/") {
            return RuntimePaths(home: URL(fileURLWithPath: home, isDirectory: true))
        }
        return RuntimePaths(home: FileManager.default.homeDirectoryForCurrentUser)
    }

    /// `~/Library/Application Support/Pliwee` — identity state.
    public var dataDirectory: URL {
        home.appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Application Support", isDirectory: true)
            .appendingPathComponent(Self.appDirectory, isDirectory: true)
    }

    /// The control socket's directory.
    public var runDirectory: URL {
        dataDirectory.appendingPathComponent("run", isDirectory: true)
    }

    /// The control socket.
    public var controlSocket: URL {
        runDirectory.appendingPathComponent("control.sock", isDirectory: false)
    }

    /// `~/Library/Logs/Pliwee`.
    public var logsDirectory: URL {
        home.appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Logs", isDirectory: true)
            .appendingPathComponent(Self.appDirectory, isDirectory: true)
    }

    /// The agent's log when `launchd` runs it.
    public var logFile: URL {
        logsDirectory.appendingPathComponent("pliweed.log", isDirectory: false)
    }

    /// Where received files go by default: the shared `files.v1` default,
    /// `~/Downloads/Pliwee`.
    public var downloadsDirectory: URL {
        home.appendingPathComponent("Downloads", isDirectory: true)
            .appendingPathComponent(Self.appDirectory, isDirectory: true)
    }
}
