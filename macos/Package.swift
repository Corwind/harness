// swift-tools-version: 5.9
import PackageDescription

// SwiftPM package for the Harness macOS app sources.
// The `.app` is produced by xcodegen + xcodebuild via scripts/build-app.sh;
// this Package.swift exists so that `swift build` and `swift test` can run
// the Swift sources headlessly in CI and during agent work.

let package = Package(
    name: "Harness",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .library(name: "HarnessApp", targets: ["HarnessApp"]),
    ],
    dependencies: [],
    targets: [
        .target(
            name: "HarnessApp",
            path: "Sources/Harness",
            exclude: ["Resources"]
        ),
        .testTarget(
            name: "HarnessTests",
            dependencies: ["HarnessApp"],
            path: "Tests/HarnessTests"
        ),
    ]
)
