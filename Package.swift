// swift-tools-version: 5.8
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "Pinenut",
    platforms: [.iOS(.v13)],
    products: [
        .library(
            name: "Pinenut",
            targets: ["Pinenut"]
        ),
    ],
    targets: [
        .target(
            name: "Pinenut",
            dependencies: ["PinenutFFI"],
            path: "ios/Pinenut",
            sources: ["Pinenut.swift", "Pinenut+Convenience.swift"]
        ),
        .binaryTarget(
            name: "PinenutFFI",
            path: "ios/PinenutFFI.xcframework"
        ),
        .testTarget(
            name: "Benchmark",
            dependencies: ["Pinenut"],
            path: "ios/Benchmark"
        ),
    ]
)
