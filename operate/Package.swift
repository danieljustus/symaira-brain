// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "symaira-operate",
    platforms: [
        .macOS(.v26),
    ],
    products: [
        .library(name: "SymOperateCore", targets: ["SymOperateCore"]),
        .library(name: "SymOperateMCP", targets: ["SymOperateMCP"]),
        .library(name: "SymOperateCLI", targets: ["SymOperateCLI"]),
        .executable(name: "symoperate", targets: ["symoperate"]),
        .executable(name: "symoperate-freeform-snapshot", targets: ["symoperate-freeform-snapshot"]),
    ],
    dependencies: [
        .package(url: "https://github.com/danieljustus/symaira-corekit.git", exact: "0.20.0"),
        .package(path: "../history"),
    ],
    targets: [
        .target(name: "FreeformSnapshotCore", linkerSettings: [.linkedLibrary("sqlite3")]),
        .executableTarget(name: "symoperate-freeform-snapshot", dependencies: ["FreeformSnapshotCore"]),
        .target(
            name: "SymOperateCore",
            dependencies: [
                "FreeformSnapshotCore",
                .product(name: "SymCockpitHistory", package: "history"),
                .product(name: "SymairaUpdateCheck", package: "symaira-corekit"),
            ],
            resources: [.process("Resources")],
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
            ]
        ),
        .target(
            name: "SymOperateMCP",
            dependencies: [
                "SymOperateCore",
                .product(name: "SymairaMCP", package: "symaira-corekit"),
            ]
        ),
        .target(
            name: "SymOperateCLI",
            dependencies: ["SymOperateCore", "SymOperateMCP"]
        ),
        .executableTarget(
            name: "symoperate",
            dependencies: ["SymOperateCLI"]
        ),
        .testTarget(
            name: "SymOperateCoreTests",
            dependencies: [
                "SymOperateCore",
                "SymOperateMCP",
                .product(name: "SymairaUpdateCheck", package: "symaira-corekit"),
                .product(name: "SymairaMCP", package: "symaira-corekit"),
            ]
        ),
        .testTarget(
            name: "SymOperateSmokeTests",
            dependencies: ["SymOperateCore", "SymOperateMCP"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
