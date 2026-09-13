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
    ],
    dependencies: [
        .package(url: "https://github.com/danieljustus/symaira-appkit.git", exact: "0.14.2"),
        .package(path: "../history"),
    ],
    targets: [
        .target(
            name: "SymOperateCore",
            dependencies: [
                .product(name: "SymCockpitHistory", package: "history"),
                .product(name: "SymairaUpdateCheck", package: "symaira-appkit"),
            ],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
            ]
        ),
        .target(
            name: "SymOperateMCP",
            dependencies: [
                "SymOperateCore",
                .product(name: "SymairaMCP", package: "symaira-appkit"),
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
                .product(name: "SymairaUpdateCheck", package: "symaira-appkit"),
                .product(name: "SymairaMCP", package: "symaira-appkit"),
            ]
        ),
        .testTarget(
            name: "SymOperateSmokeTests",
            dependencies: ["SymOperateCore", "SymOperateMCP"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
