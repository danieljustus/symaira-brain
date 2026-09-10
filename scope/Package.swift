// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "symaira-scope",
    platforms: [
        .macOS(.v26),
    ],
    products: [
        .library(name: "SymScopeCore", targets: ["SymScopeCore"]),
        .library(name: "SymScopeMCP", targets: ["SymScopeMCP"]),
        .library(name: "SymScopeCLI", targets: ["SymScopeCLI"]),
        .executable(name: "symscope", targets: ["symscope"]),
    ],
    dependencies: [
        .package(url: "https://github.com/danieljustus/symaira-appkit.git", exact: "0.14.2"),
        .package(path: "../history"),
    ],
    targets: [
        .target(
            name: "SymScopeCore",
            dependencies: [
                .product(name: "SymCockpitHistory", package: "history"),
            ]
        ),
        .target(
            name: "SymScopeMCP",
            dependencies: [
                "SymScopeCore",
                .product(name: "SymairaMCP", package: "symaira-appkit"),
            ]
        ),
        .target(
            name: "SymScopeCLI",
            dependencies: [
                "SymScopeCore",
                "SymScopeMCP",
            ]
        ),
        .executableTarget(
            name: "symscope",
            dependencies: [
                "SymScopeCLI"
            ]
        ),
        .testTarget(
            name: "SymScopeCoreTests",
            dependencies: ["SymScopeCore"]
        ),
        .testTarget(
            name: "SymScopeMCPTests",
            dependencies: ["SymScopeMCP"]
        ),
    ],
    swiftLanguageModes: [.v6]
)
