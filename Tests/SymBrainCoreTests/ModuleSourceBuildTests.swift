import Foundation
import Testing
@testable import SymBrainCore

@Suite("Module source checkout")
struct ModuleSourceBuildTests {
    @Test func canonicalizesAndValidatesAWorktreeRoot() throws {
        let workspace = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: workspace) }

        let checkout = workspace.appendingPathComponent("brain", isDirectory: true)
        try createCheckout(at: checkout, gitMetadata: .worktreeFile)
        let alias = workspace.appendingPathComponent("checkout-alias", isDirectory: true)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: checkout)

        let validated = try ModuleSourceBuild.canonicalSourceRoot(validating: alias)

        #expect(validated.path == checkout.standardizedFileURL.resolvingSymlinksInPath().standardizedFileURL.path)
    }

    @Test func rejectsCheckoutMissingRequiredModuleMarker() throws {
        let workspace = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: workspace) }

        let checkout = workspace.appendingPathComponent("brain", isDirectory: true)
        try createCheckout(at: checkout, gitMetadata: .directory)
        try FileManager.default.removeItem(at: checkout.appendingPathComponent("scope/Package.swift"))

        do {
            _ = try ModuleSourceBuild.canonicalSourceRoot(validating: checkout)
            Issue.record("Expected a checkout missing scope/Package.swift to be rejected.")
        } catch let error as ModuleSourceBuild.ValidationError {
            #expect(error == .missingFile("scope/Package.swift"))
        }
    }

    @Test func buildsSetupArgumentsInCLIOrder() {
        let sourceRoot = URL(fileURLWithPath: "/Volumes/Symaira Brain/checkout")

        #expect(
            ModuleSourceBuild.setupArguments(module: "scope", sourceRoot: sourceRoot)
                == ["setup", "--from-source", sourceRoot.path, "--modules", "scope", "--json"]
        )
        #expect(
            ModuleSourceBuild.copyableSetupCommand(module: "scope", sourceRoot: sourceRoot)
                == "symbrain setup --from-source '/Volumes/Symaira Brain/checkout' --modules scope --json"
        )
        #expect(
            ModuleSourceBuild.copyableSetupCommand(module: "operate", sourceRoot: nil)
                == "symbrain setup --from-source '<select-brain-source-checkout>' --modules operate --json"
        )
    }

    @Test func quotesApostrophesInCopyableSourcePath() {
        let sourceRoot = URL(fileURLWithPath: "/Volumes/Daniel's Brain/checkout")

        #expect(
            ModuleSourceBuild.copyableSetupCommand(module: "operate", sourceRoot: sourceRoot)
                == "symbrain setup --from-source '/Volumes/Daniel'\"'\"'s Brain/checkout' --modules operate --json"
        )
    }

    private enum GitMetadata {
        case directory
        case worktreeFile
    }

    private func createCheckout(at root: URL, gitMetadata: GitMetadata) throws {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: root, withIntermediateDirectories: true)
        try fileManager.createDirectory(
            at: root.appendingPathComponent("browse", isDirectory: true),
            withIntermediateDirectories: true
        )
        for path in ["go.mod", "operate/Package.swift", "scope/Package.swift", "history/Package.swift"] {
            let file = root.appendingPathComponent(path)
            try fileManager.createDirectory(
                at: file.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try Data("marker".utf8).write(to: file)
        }
        let gitMetadataURL = root.appendingPathComponent(".git")
        switch gitMetadata {
        case .directory:
            try fileManager.createDirectory(at: gitMetadataURL, withIntermediateDirectories: true)
        case .worktreeFile:
            try Data("gitdir: /tmp/brain/.git/worktrees/test\n".utf8).write(to: gitMetadataURL)
        }
    }
}
