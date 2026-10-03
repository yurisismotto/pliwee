// Pliwee's design tokens, as values.
//
// `docs/design/tokens.json` is the single source of truth, and it is read by
// a test on every platform: Android's `DesignTokensTest`, the GTK crate's
// token tests, and `DesignTokensTests` here. Change a value there and this
// table fails until it matches. Kept free of SwiftUI so the test needs no
// display; the app turns these into dynamic colours.
//
// Brand hues are for fills, marks and indicator dots. Text and small icons
// use the `onLight`/`onDark` corrections, which reach WCAG AA — Flow Cyan is
// 2.40:1 on white and may never be a text colour (docs/design/BRAND.md).

import Foundation

public enum DesignTokens {
    public enum Brand {
        public static let cyan = "#18B8C9"
        public static let blue = "#4F6BFF"
        public static let violet = "#7C5CFC"
        public static let dark = "#0B1020"
        public static let surface = "#F7F9FC"
    }

    /// A colour with a light-appearance and a dark-appearance value.
    public struct Pair: Equatable, Sendable {
        public let light: String
        public let dark: String
    }

    /// Brand hues corrected for text and icons.
    public enum Text {
        public static let cyan = Pair(light: "#10747E", dark: "#3DC9D7")
        public static let blue = Pair(light: "#445CDD", dark: "#90A1FF")
        public static let violet = Pair(light: "#6A49EE", dark: "#A28CFA")
        public static let amber = Pair(light: "#B45309", dark: "#FBBF24")
        public static let red = Pair(light: "#DC2626", dark: "#F87171")
    }

    /// Status colours. Never carried by colour alone: every status is also an
    /// icon and a word (docs/design/UI-GUIDELINES.md).
    public enum Status {
        public static let connected = Pair(light: "#10747E", dark: "#3DC9D7")
        public static let available = Pair(light: "#445CDD", dark: "#90A1FF")
        public static let transferring = Pair(light: "#445CDD", dark: "#90A1FF")
        public static let success = Pair(light: "#10747E", dark: "#3DC9D7")
        public static let warning = Pair(light: "#B45309", dark: "#FBBF24")
        public static let error = Pair(light: "#DC2626", dark: "#F87171")
        public static let stale = Pair(light: "#B45309", dark: "#FBBF24")
    }

    /// `#RRGGBB` as components in 0…1.
    public static func rgb(_ hex: String) -> (red: Double, green: Double, blue: Double)? {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        return (
            Double((value >> 16) & 0xFF) / 255,
            Double((value >> 8) & 0xFF) / 255,
            Double(value & 0xFF) / 255
        )
    }
}
