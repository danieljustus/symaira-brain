import Foundation
import Testing
@testable import SymBrainCore

/// Tests for SyncViewModel sync-mode semantics (#148, #147).
@MainActor
struct SyncViewModelTests {
    /// A client that can never locate the CLI binary, so every sync()
    /// attempt fails fast and observably (isBinaryNotFound/errorMessage)
    /// without touching the filesystem.
    private func makeClient() -> SymBrainClient {
        SymBrainClient(searchPATH: "", extraDirectories: [])
    }

    private func makeSummary() throws -> SyncSummary {
        let json = """
        {"targets": [{"name": "claude", "path": "./CLAUDE.md", "status": "dry-run", "message": "would update"}], "skills": []}
        """
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(SyncSummary.self, from: Data(json.utf8))
    }

    // MARK: - #148: dry-run toggle must never trigger a live sync

    @Test func leavingDryRunOnlyClearsPreviewAndNeverSyncs() async throws {
        let vm = SyncViewModel(client: makeClient())
        vm.syncSummary = try makeSummary()

        await vm.dryRunChanged(to: false)

        // Stale preview cleared...
        #expect(vm.syncSummary == nil)
        // ...and no sync attempt happened. A sync() attempt would surface
        // the missing-binary error on this client.
        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
        #expect(vm.isLoading == false)
    }

    @Test func enteringDryRunRefreshesPreview() async {
        let vm = SyncViewModel(client: makeClient())

        await vm.dryRunChanged(to: true)

        // A sync attempt was made (it fails here only because no binary
        // exists), proving entering dry-run refreshes the preview.
        #expect(vm.errorMessage != nil)
        #expect(vm.isBinaryNotFound == true)
    }

    @Test func firstLiveSyncRequiresConfirmation() async {
        let vm = SyncViewModel(client: makeClient())

        // Dry-run previews are always safe and run immediately.
        #expect(vm.dryRun == true)
        #expect(vm.canSyncImmediately == true)
        #expect(vm.liveSyncConfirmed == false)

        // Leaving dry-run changes the mode only; live sync now needs
        // an explicit confirmation before it may run.
        vm.dryRun = false
        #expect(vm.canSyncImmediately == false)

        // One confirmation per session unlocks live sync.
        vm.confirmLiveSync()
        #expect(vm.liveSyncConfirmed == true)
        #expect(vm.canSyncImmediately == true)
    }

    @Test func clearPreviewResetsErrorStateToo() async {
        let vm = SyncViewModel(client: makeClient())
        await vm.sync() // fails: binary missing

        #expect(vm.errorMessage != nil)
        #expect(vm.isBinaryNotFound == true)

        vm.clearPreview()

        #expect(vm.syncSummary == nil)
        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
    }

    // MARK: - #147: target path display

    @Test func displayPathResolvesRelativeTargetsAgainstWorkingDirectory() {
        let vm = SyncViewModel(client: makeClient())
        let cwd = FileManager.default.currentDirectoryPath

        let rootTarget = SyncTargetStatus(
            name: "claude",
            path: "./CLAUDE.md",
            status: "dry-run",
            message: nil
        )
        let resolvedRoot = vm.displayPath(for: rootTarget)
        #expect(resolvedRoot.hasPrefix("/"))
        #expect(resolvedRoot.hasPrefix(cwd))
        #expect(resolvedRoot.hasSuffix("/CLAUDE.md"))

        let nestedTarget = SyncTargetStatus(
            name: "cursor",
            path: ".cursor/rules/symbrain.mdc",
            status: "dry-run",
            message: nil
        )
        let resolvedNested = vm.displayPath(for: nestedTarget)
        #expect(resolvedNested.hasPrefix(cwd))
        #expect(resolvedNested.hasSuffix("/.cursor/rules/symbrain.mdc"))
    }

    @Test func displayPathPassesThroughAbsolutePaths() {
        let vm = SyncViewModel(client: makeClient())
        let target = SyncTargetStatus(
            name: "agents",
            path: "/path/.agents.md",
            status: "updated",
            message: nil
        )

        #expect(vm.displayPath(for: target) == "/path/.agents.md")
    }
}
