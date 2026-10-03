// The Pliwee artwork, from `docs/design/assets/`, as the bundle carries it.
//
// `scripts/build-app.sh` copies the brand SVGs into `Contents/Resources`
// unchanged — no new logo, no redrawn mark — and AppKit draws them directly.
// Run outside a bundle (`swift run`) there are no resources, and an SF
// Symbol stands in so the menu-bar item still exists.

import AppKit
import PliweeKit
import SwiftUI

enum BrandAssets {
    /// The mono mark, as a template image sized for the menu bar.
    static func menuBarIcon(active: Bool) -> NSImage {
        let image: NSImage
        if let url = Bundle.main.url(forResource: "pliwee-mark-mono", withExtension: "svg"),
           let svg = NSImage(contentsOf: url) {
            // 276×255 artwork; 18 pt tall is the menu bar's standard height.
            let height: CGFloat = 16
            let size = NSSize(width: svg.size.width / svg.size.height * height, height: height)
            image = NSImage(size: size, flipped: false) { rect in
                svg.draw(in: rect, from: .zero, operation: .sourceOver, fraction: active ? 1 : 0.45)
                return true
            }
        } else {
            image = NSImage(systemSymbolName: "dot.radiowaves.left.and.right", accessibilityDescription: "Pliwee")
                ?? NSImage()
        }
        image.isTemplate = true
        return image
    }

    /// The full-colour mark, for the window.
    static var mark: NSImage? {
        Bundle.main.url(forResource: "pliwee-mark", withExtension: "svg").flatMap(NSImage.init(contentsOf:))
    }
}

extension Color {
    /// A token pair as a colour that follows the system appearance.
    static func token(_ pair: DesignTokens.Pair) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let dark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return NSColor(hex: dark ? pair.dark : pair.light)
        })
    }

    static let pliweeBlue = Color(nsColor: NSColor(hex: DesignTokens.Brand.blue))
    static let pliweeCyan = Color(nsColor: NSColor(hex: DesignTokens.Brand.cyan))
    static let pliweeViolet = Color(nsColor: NSColor(hex: DesignTokens.Brand.violet))
}

extension NSColor {
    convenience init(hex: String) {
        let rgb = DesignTokens.rgb(hex) ?? (0, 0, 0)
        self.init(srgbRed: rgb.red, green: rgb.green, blue: rgb.blue, alpha: 1)
    }
}
