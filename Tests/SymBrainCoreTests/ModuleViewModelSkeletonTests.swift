import Foundation
import Testing
@testable import SymBrainCore
import SymairaCLIRunner

/// Tests for the shared module view model skeleton (#238).
///
/// Verifies that `clearError()` and `report(_:)` — provided by
/// `ModuleViewModelProtocol` — behave consistently across all three
/// module view models.
@MainActor
struct ModuleViewModelSkeletonTests {

    // MARK: - clearError()

    @Test func memoryViewModelClearErrorResetsAllState() async {
        let vm = MemoryViewModel()
        vm.errorMessage = "oops"
        vm.errorDetail = "detail"
        vm.isBinaryNotFound = true
        vm.statusMessage = "saved"

        vm.clearError()

        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
        // statusMessage is NOT part of clearError — it persists.
        #expect(vm.statusMessage == "saved")
    }

    @Test func vaultViewModelClearErrorResetsAllState() async {
        let vm = VaultViewModel()
        vm.errorMessage = "oops"
        vm.errorDetail = "detail"
        vm.isBinaryNotFound = true
        vm.statusMessage = "unlocked"

        vm.clearError()

        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
        #expect(vm.statusMessage == "unlocked")
    }

    @Test func skillsViewModelClearErrorResetsAllState() async {
        let vm = SkillsViewModel()
        vm.errorMessage = "oops"
        vm.errorDetail = "detail"
        vm.isBinaryNotFound = true
        vm.statusMessage = "synced"

        vm.clearError()

        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
        #expect(vm.statusMessage == "synced")
    }

    // MARK: - report(_:)

    @Test func reportGenericErrorSetsMessageAndDetail() async {
        let vm = MemoryViewModel()
        let error = NSError(domain: "test", code: 42, userInfo: [
            NSLocalizedDescriptionKey: "something broke"
        ])

        vm.report(error)

        #expect(vm.errorMessage != nil)
        #expect(vm.errorMessage!.contains("something broke")
                || vm.errorMessage!.contains("42"))
        #expect(vm.isBinaryNotFound == false)
    }

    @Test func reportBinaryNotFoundSetsFlag() async {
        let vm = MemoryViewModel()
        let error = CLIRunnerError.binaryNotFound(tool: "symmemory")

        vm.report(error)

        #expect(vm.isBinaryNotFound == true)
        #expect(vm.errorMessage != nil)
        // The CLIErrorFormatter message mentions the tool name.
        #expect(vm.errorMessage!.contains("symmemory"))
    }

    @Test func vaultReportBinaryNotFoundSetsFlag() async {
        let vm = VaultViewModel()
        let error = CLIRunnerError.binaryNotFound(tool: "symvault")

        vm.report(error)

        // This was the missing branch in VaultViewModel's old report().
        #expect(vm.isBinaryNotFound == true)
        #expect(vm.errorMessage != nil)
        #expect(vm.errorMessage!.contains("symvault"))
    }

    @Test func skillsReportBinaryNotFoundSetsFlag() async {
        let vm = SkillsViewModel()
        let error = CLIRunnerError.binaryNotFound(tool: "symskills")

        vm.report(error)

        #expect(vm.isBinaryNotFound == true)
        #expect(vm.errorMessage != nil)
        #expect(vm.errorMessage!.contains("symskills"))
    }

    @Test func reportExecutionFailedSetsMessageAndDetail() async {
        let vm = SkillsViewModel()
        let error = CLIRunnerError.executionFailed(code: 1, fullStderr: "bad input")

        vm.report(error)

        #expect(vm.errorMessage != nil)
        #expect(vm.errorDetail != nil)
        #expect(vm.isBinaryNotFound == false)
    }

    // MARK: - clearError after report

    @Test func clearErrorAfterReportResetsEverything() async {
        let vm = SkillsViewModel()
        vm.report(CLIRunnerError.binaryNotFound(tool: "symskills"))

        #expect(vm.isBinaryNotFound == true)
        #expect(vm.errorMessage != nil)

        vm.clearError()

        #expect(vm.errorMessage == nil)
        #expect(vm.errorDetail == nil)
        #expect(vm.isBinaryNotFound == false)
    }
}

#if os(macOS)
private struct StubVaultClient: VaultClientProtocol {
    let result: VaultEntryDetail
    var isInstalled: Bool { true }
    func availability(profile: String?) async -> VaultAvailability { .ready }
    func version(profile: String?) async throws -> String { "test" }
    func unlock(passphrase: String, ttl: String, profile: String?) async throws {}
    func lock(profile: String?) async throws {}
    func list(profile: String?) async throws -> [VaultEntrySummary] { [] }
    func find(query: String, profile: String?) async throws -> [VaultEntrySummary] { [] }
    func entry(path: String, profile: String?) async throws -> VaultEntryDetail { result }
}

@MainActor
private final class ControlledVaultClient: VaultClientProtocol {
    var isInstalled = true
    var pending: [(path: String, continuation: CheckedContinuation<VaultEntryDetail, Never>)] = []

    func availability(profile: String?) async -> VaultAvailability { .ready }
    func version(profile: String?) async throws -> String { "test" }
    func unlock(passphrase: String, ttl: String, profile: String?) async throws {}
    func lock(profile: String?) async throws {}
    func list(profile: String?) async throws -> [VaultEntrySummary] { [] }
    func find(query: String, profile: String?) async throws -> [VaultEntrySummary] { [] }

    func entry(path: String, profile: String?) async throws -> VaultEntryDetail {
        await withCheckedContinuation { continuation in
            pending.append((path, continuation))
        }
    }

    func resolve(path: String, detail: VaultEntryDetail) {
        guard let index = pending.firstIndex(where: { $0.path == path }) else { return }
        pending.remove(at: index).continuation.resume(returning: detail)
    }
}

private actor ManualSleeper {
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func sleep(_ duration: Duration) async throws {
        await withCheckedContinuation { continuation in
            waiters.append(continuation)
        }
    }

    func resumeNext() {
        guard !waiters.isEmpty else { return }
        waiters.removeFirst().resume()
    }
}

@MainActor
extension ModuleViewModelSkeletonTests {
    @Test func staleRevealAfterSelectionCannotPublishPlaintext() async {
        let client = ControlledVaultClient()
        let first = VaultEntryDetail(path: "work/first", modified: nil, fields: ["password": .string("first-secret")])
        let vm = VaultViewModel(client: client)
        vm.availability = .ready
        await vm.select(path: first.path)

        let reveal = Task { await vm.revealSelectedEntry() }
        await Task.yield()
        #expect(client.pending.map(\.path) == [first.path])

        await vm.select(path: "work/second")
        client.resolve(path: first.path, detail: first)
        await reveal.value

        #expect(vm.selectedPath == "work/second")
        #expect(vm.detail == nil)
    }

    @Test func staleRevealAfterRelockCannotPublishPlaintext() async {
        let client = ControlledVaultClient()
        let first = VaultEntryDetail(path: "work/first", modified: nil, fields: ["password": .string("first-secret")])
        let vm = VaultViewModel(client: client)
        vm.availability = .ready
        await vm.select(path: first.path)

        let reveal = Task { await vm.revealSelectedEntry() }
        await Task.yield()
        vm.availability = .locked
        client.resolve(path: first.path, detail: first)
        await reveal.value

        #expect(vm.detail == nil)
        #expect(vm.revealedFields.isEmpty)
    }

    @Test func revealExpiresWhenControllableClockAdvances() async {
        let sleeper = ManualSleeper()
        let detail = VaultEntryDetail(path: "work/test", modified: nil, fields: ["password": .string("secret")])
        let vm = VaultViewModel(
            client: StubVaultClient(result: detail),
            sleep: { duration in try await sleeper.sleep(duration) }
        )
        vm.availability = .ready
        await vm.select(path: detail.path)
        await vm.revealSelectedEntry()
        await Task.yield()
        #expect(vm.detail == detail)

        await sleeper.resumeNext()
        await Task.yield()
        #expect(vm.detail == nil)
        #expect(vm.revealedFields.isEmpty)
    }

    @Test func copyFieldRequiresRevealAndMarksSecretIntent() async {
        let detail = VaultEntryDetail(path: "work/test", modified: nil, fields: [:])
        var copies: [(value: String, concealed: Bool)] = []
        let vm = VaultViewModel(
            client: StubVaultClient(result: detail),
            clipboardWriter: { value, concealed in copies.append((value, concealed)) }
        )

        vm.copyField("password", value: "secret", revealed: false)
        #expect(copies.isEmpty)
        vm.copyField("password", value: "secret", revealed: true)
        vm.copyField("username", value: "daniel", revealed: false)

        #expect(copies.map(\.concealed) == [true, false])
        #expect(copies.map(\.value) == ["secret", "daniel"])
    }
}
#endif
