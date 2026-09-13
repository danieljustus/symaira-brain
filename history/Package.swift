// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "symcockpit-history",
    platforms: [.macOS(.v26)],
    products: [
        .library(name: "SymCockpitHistory", targets: ["SymCockpitHistory"]),
    ],
    targets: [
        .target(name: "SymCockpitHistory"),
        .testTarget(
            name: "SymCockpitHistoryTests",
            dependencies: ["SymCockpitHistory"],
            resources: [.copy("Fixtures")]
        ),
    ],
    swiftLanguageModes: [.v6]
)
