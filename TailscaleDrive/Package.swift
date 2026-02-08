// swift-tools-version: 6.2

// Package.swift
import PackageDescription
import Foundation

let root = FileManager.default.currentDirectoryPath

let package = Package(
  name: "TailscaleDrive",
  platforms: [.iOS(.v17)],
  products: [
    .library(name: "TailscaleDrive", targets: ["TailscaleDrive"]),
  ],
  targets: [
    .target(
      name: "BridgeFFI",
      path: "Sources/BridgeFFI",
      publicHeadersPath: "include"
    ),
    .target(
      name: "TailscaleDrive",
      dependencies: ["BridgeFFI"],
      linkerSettings: [
        .unsafeFlags(["-L", "\(root)/RustLibs"]),
        .linkedLibrary("tailscale_drive"),
      ]
    ),
  ]
)
