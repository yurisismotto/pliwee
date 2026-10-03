// One Pliwee.app per user session.
//
// macOS treats two copies of an app at different paths as two applications:
// opening the second — from Launchpad or Spotlight, which list every copy —
// starts a second process while the first is running. For a menu-bar app
// that means two icons and, worse, two clients attaching to the agent as the
// incoming-file approval provider, each showing its own prompt. Measured with
// a physical Android peer during the closure round.
//
// The rule: the newcomer yields to the instance that was already running,
// asks it to show its window, and quits before it has touched the agent.

import Foundation

public enum SingleInstance {
    /// A running process of this application, as `NSRunningApplication`
    /// describes it.
    public struct Instance: Equatable, Sendable {
        public let pid: Int32
        public let launched: Date?

        public init(pid: Int32, launched: Date?) {
            self.pid = pid
            self.launched = launched
        }
    }

    /// The instance this process should hand over to, or `nil` when it is the
    /// one that should keep running.
    ///
    /// Any other instance wins over this one: the newcomer always yields. When
    /// several others exist, the earliest launched is the one to keep, so two
    /// newcomers cannot each pick a different survivor.
    public static func instanceToYieldTo(selfPID: Int32, running: [Instance]) -> Instance? {
        running
            .filter { $0.pid != selfPID }
            .min { ($0.launched ?? .distantFuture, $0.pid) < ($1.launched ?? .distantFuture, $1.pid) }
    }
}
