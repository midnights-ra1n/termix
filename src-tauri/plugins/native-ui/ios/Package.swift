// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "tauri-plugin-native-ui",
  platforms: [.iOS(.v15)],
  products: [
    .library(name: "tauri-plugin-native-ui", type: .static, targets: ["tauri-plugin-native-ui"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(name: "tauri-plugin-native-ui", dependencies: [.byName(name: "Tauri")], path: "Sources")
  ]
)
