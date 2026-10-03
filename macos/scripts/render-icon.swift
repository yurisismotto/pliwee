// Renders the Pliwee app icon into a macOS `.iconset`.
//
//   swift render-icon.swift <pliwee-app-icon.svg> <out.iconset>
//
// The artwork is `docs/design/assets/pliwee-app-icon.svg`, unchanged: this
// draws it at the ten sizes `iconutil` expects and nothing else. No new logo
// and no committed bitmaps — the bundle's icon is derived from the one
// source the Linux and Android builds use.

import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write(Data("usage: render-icon.swift <svg> <out.iconset>\n".utf8))
    exit(2)
}
let source = URL(fileURLWithPath: arguments[1])
let output = URL(fileURLWithPath: arguments[2], isDirectory: true)

guard let svg = NSImage(contentsOf: source), svg.size.width > 0 else {
    FileHandle.standardError.write(Data("render-icon: cannot read \(source.path)\n".utf8))
    exit(1)
}
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

// name, pixel size
let sizes: [(String, Int)] = [
    ("icon_16x16", 16), ("icon_16x16@2x", 32),
    ("icon_32x32", 32), ("icon_32x32@2x", 64),
    ("icon_128x128", 128), ("icon_128x128@2x", 256),
    ("icon_256x256", 256), ("icon_256x256@2x", 512),
    ("icon_512x512", 512), ("icon_512x512@2x", 1024),
]

for (name, pixels) in sizes {
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ) else { exit(1) }
    rep.size = NSSize(width: pixels, height: pixels)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    svg.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels))
    NSGraphicsContext.restoreGraphicsState()
    guard let png = rep.representation(using: .png, properties: [:]) else { exit(1) }
    try png.write(to: output.appendingPathComponent("\(name).png"))
}
print("render-icon: \(sizes.count) sizes from \(source.lastPathComponent)")
