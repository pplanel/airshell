// swift-tools-version: 6.2
// Wi-Fi Aware is Swift-only and iOS/iPadOS-only; this package exposes it to the
// Rust crate (`src/wifi_aware.rs`) as blocking C functions.

import PackageDescription

let package = Package(
    name: "AirshellWiFiAware",
    platforms: [.iOS(.v26)],
    products: [
        .library(name: "AirshellWiFiAware", type: .static, targets: ["AirshellWiFiAware"]),
    ],
    targets: [
        .target(name: "AirshellWiFiAware"),
    ]
)
