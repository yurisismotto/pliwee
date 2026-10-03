// swift-tools-version:5.10
//
// Pliwee for macOS: a native SwiftUI client of the Pliwee agent (`pliweed`).
//
// A Swift package rather than an `.xcodeproj` so that it builds with nothing
// but the Command Line Tools — `swift build`, `swift test` — and opens
// unchanged in Xcode (File › Open… › this directory). `scripts/build-app.sh`
// assembles the `.app` bundle around the executable this produces.
//
//   PliweeKit   everything testable: the control-protocol models, the
//               Unix-socket client, runtime paths, and the view-independent
//               rules (device state, action availability, service health).
//               No SwiftUI, no AppKit.
//   Pliwee      the application: SwiftUI scenes, the menu-bar item, AppKit
//               where SwiftUI has no equivalent, ServiceManagement.

import PackageDescription

let package = Package(
    name: "Pliwee",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "Pliwee", targets: ["Pliwee"]),
    ],
    targets: [
        .target(
            name: "PliweeKit",
            path: "Sources/PliweeKit"
        ),
        .executableTarget(
            name: "Pliwee",
            dependencies: ["PliweeKit"],
            path: "Sources/Pliwee"
        ),
        .testTarget(
            name: "PliweeKitTests",
            dependencies: ["PliweeKit"],
            path: "Tests/PliweeKitTests"
        ),
    ]
)
