// swift-tools-version:5.9
//
// One SwiftUI codebase for the macOS and iOS shells (ADR-0001, ADR-0008).
//
// Build the Rust core first: `scripts/build-xcframework.sh` (repo root)
// writes Frameworks/Player5Core.xcframework, a static library plus the
// cbindgen header and a module map. Then:
//
//   swift build && swift test        # library, macOS executable, tests
//   swift run Player5Mac             # run the macOS app from the package
//
// apps/ios/project.yml consumes the `Player5Kit` product as a local package;
// apps/mac/project.yml builds a distributable, sandboxed macOS .app.

import PackageDescription

let package = Package(
    name: "Player5Kit",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
    ],
    products: [
        .library(name: "Player5Kit", targets: ["Player5Kit"]),
        .executable(name: "Player5Mac", targets: ["Player5Mac"]),
    ],
    targets: [
        // The Rust core (core/ffi): C ABI, lib name `player5`.
        .binaryTarget(
            name: "Player5Core",
            path: "Frameworks/Player5Core.xcframework"
        ),
        // Engine wrapper, pattern model, clock, audio I/O and SwiftUI views.
        .target(
            name: "Player5Kit",
            dependencies: ["Player5Core"]
        ),
        // `swift run Player5Mac`; the XcodeGen project builds the same file
        // into a signed, sandboxed .app.
        .executableTarget(
            name: "Player5Mac",
            dependencies: ["Player5Kit"]
        ),
        .testTarget(
            name: "Player5KitTests",
            dependencies: ["Player5Kit", "Player5Core"]
        ),
    ]
)
