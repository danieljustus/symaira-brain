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
    func create(path: String, value: String, profile: String?) async throws -> VaultCreateConfirmation {
        VaultCreateConfirmation(submittedPath: path, confirmedPath: path, confirmedFieldCount: 1, confirmedHasValue: true)
    }
    func set(path: String, field: String, value: String, profile: String?) async throws -> VaultSetConfirmation {
        VaultSetConfirmation(submittedPath: path, submittedField: field, confirmedPath: path, confirmedField: field, confirmedValueMatches: true, confirmedFieldCount: 1, confirmedHasValue: true)
    }
    func delete(path: String, profile: String?) async throws -> VaultDeleteConfirmation {
        VaultDeleteConfirmation(submittedPath: path, confirmedPath: path, confirmedAbsent: true)
    }
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

    func create(path: String, value: String, profile: String?) async throws -> VaultCreateConfirmation {
        VaultCreateConfirmation(submittedPath: path, confirmedPath: path, confirmedFieldCount: 1, confirmedHasValue: true)
    }
    func set(path: String, field: String, value: String, profile: String?) async throws -> VaultSetConfirmation {
        VaultSetConfirmation(submittedPath: path, submittedField: field, confirmedPath: path, confirmedField: field, confirmedValueMatches: true, confirmedFieldCount: 1, confirmedHasValue: true)
    }
    func delete(path: String, profile: String?) async throws -> VaultDeleteConfirmation {
        VaultDeleteConfirmation(submittedPath: path, confirmedPath: path, confirmedAbsent: true)
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
    @Test func createEntryClearsValueAndPublishesSanitizedConfirmation() async {
        let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "x", modified: nil, fields: [:])))
        vm.availability = .ready
        vm.createPath = "work/new"
        vm.createValue = "do-not-retain"
        await vm.createEntry()
        #expect(vm.createValue.isEmpty)
        #expect(vm.createConfirmation?.submittedPath == "work/new")
        #expect(vm.createConfirmation?.confirmedHasValue == true)
    }

    @Test func staleRevealAfterSelectionCannotPublishPlaintext() async {
        let client = ControlledVaultClient()
        let first = VaultEntryDetail(path: "work/first", modified: nil, fields: ["password": .string("first-secret")])
        let vm = VaultViewModel(client: client)
        vm.availability = .ready
        await vm.select(path: first.path)

        let reveal = Task { await vm.revealSelectedEntry() }
        for _ in 0..<100 {
            if client.pending.map(\.path) == [first.path] { break }
            await Task.yield()
        }
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
        for _ in 0..<100 {
            if client.pending.map(\.path) == [first.path] { break }
            await Task.yield()
        }
        #expect(client.pending.map(\.path) == [first.path])
        guard client.pending.map(\.path) == [first.path] else {
            reveal.cancel()
            return
        }
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

    @Test func copyAPIGuardsMaskedStaleAndMissingDetails() async {
        let detail = VaultEntryDetail(
            path: "work/test",
            modified: nil,
            fields: ["password": .string("secret"), "username": .string("daniel")],
            totp: VaultTOTP(code: "123456", period: 30, remaining: 20)
        )
        var copies: [(value: String, concealed: Bool)] = []
        let vm = VaultViewModel(
            client: StubVaultClient(result: detail),
            clipboardWriter: { value, concealed in copies.append((value, concealed)) }
        )
        vm.availability = .ready

        await vm.select(path: detail.path)
        vm.copyField("password")
        vm.copyTOTP()
        #expect(copies.isEmpty)

        await vm.revealSelectedEntry()
        vm.copyField("password")
        vm.copyTOTP()
        #expect(copies.map(\.concealed) == [true])
        #expect(copies.map(\.value) == ["123456"])

        vm.toggleReveal(field: "password")
        vm.copyField("password")
        vm.copyField("username")
        vm.copyTOTP()
        #expect(copies.map(\.concealed) == [true, true, false, true])
        #expect(copies.map(\.value) == ["123456", "secret", "daniel", "123456"])

        await vm.select(path: "work/other")
        vm.copyField("password")
        vm.copyTOTP()
        #expect(copies.count == 4)

        vm.copyInstallCommand()
        #expect(copies.last?.concealed == false)
    }
    @Test func setEntryClearsPlaintextAndPublishesConfirmation() async {
        let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "work/edit", modified: nil, fields: ["password": .string("hidden")])))
        vm.availability = .ready
        vm.selectedPath = "work/edit"
        vm.detail = VaultEntryDetail(path: "work/edit", modified: nil, fields: ["password": .string("hidden")])
        vm.editValue = "secret-never-retained"
        await vm.setSelectedEntry()
        #expect(vm.editValue.isEmpty)
        #expect(vm.editConfirmation?.confirmedPath == "work/edit")
    }

    @Test func deleteEntryClearsSelectionAfterConfirmedAbsence() async {
        let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "work/delete", modified: nil, fields: [:])))
        vm.availability = .ready
        vm.selectedPath = "work/delete"
        await vm.deleteEntry(path: "work/delete")
        #expect(vm.selectedPath == nil)
        #expect(vm.deleteConfirmation?.confirmedAbsent == true)
    }

    @Test func setSelectedEntryPreservesNonPasswordPrimaryField() async {
        let detail = VaultEntryDetail(path: "work/api", modified: nil, fields: [
            "api_key": .string("old-key"), "username": .string("daniel")
        ])
        let vm = VaultViewModel(client: StubVaultClient(result: detail))
        vm.availability = .ready
        vm.selectedPath = detail.path
        vm.detail = detail
        vm.editValue = "new-key"
        await vm.setSelectedEntry()
        #expect(vm.editConfirmation?.submittedField == "api_key")
        #expect(vm.editConfirmation?.confirmedField == "api_key")
        #expect(vm.editConfirmation?.confirmedValueMatches == true)
        #expect(vm.editValue.isEmpty)
    }


    @Test func vaultClientSubprocessRejectsMultilineBeforeDispatch() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let marker = dir.appendingPathComponent("invoked")
        let script = "#!/bin/sh\ntouch \"\(marker.path)\"\n"
        let binary = dir.appendingPathComponent("symvault")
        try script.data(using: .utf8)!.write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let client = VaultClient(userOverride: binary)
        await #expect(throws: CLIRunnerError.self) {
            _ = try await client.set(path: "work/item", field: "private_key", value: "line-one\nline-two")
        }
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }

    @Test func vaultClientRejectsCRLFCRAndLFBeforeDispatchForCreateAndSet() async throws {
        let missingBinary = URL(fileURLWithPath: "/definitely-missing-symvault-\(UUID().uuidString)")
        let client = VaultClient(userOverride: missingBinary)
        for value in ["line-one\r\nline-two", "line-one\rline-two", "line-one\nline-two"] {
            await #expect(throws: CLIRunnerError.self) {
                _ = try await client.create(path: "work/item", value: value)
            }
            await #expect(throws: CLIRunnerError.self) {
                _ = try await client.set(path: "work/item", field: "password", value: value)
            }
        }
    }

    @Test func vaultClientSubprocessValidatesAllFiveFieldsAndReadback() async throws {
        let cases = [("password", "pw"), ("api_key", "api"), ("token", "tok"), ("private_key", "key"), ("database_url", "db")]
        for (field, value) in cases {
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: dir) }
            let script = "#!/bin/sh\nif [ \"$3\" = set ]; then [ \"$4\" = \"work/item.\(field)\" ] || exit 3; tmp=\"$TMPDIR/symvault-input.$$\"; cat >\"$tmp\"; printf '%s\n' \"\(value)\" | cmp -s - \"$tmp\" || exit 4; rm -f \"$tmp\"; exit 0; fi\nif [ \"$3\" = get ]; then [ \"$4\" = work/item ] || exit 5; printf '%s' '{\"path\":\"work/item\",\"fields\":{\"\(field)\":\"\(value)\"}}'; exit 0; fi\nexit 1\n"
            let binary = dir.appendingPathComponent("symvault")
            try script.data(using: .utf8)!.write(to: binary)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
            let client = VaultClient(userOverride: binary)
            let confirmation = try await client.set(path: "work/item", field: field, value: value)
            #expect(confirmation.confirmedField == field)
            #expect(confirmation.confirmedValueMatches)
        }
    }

    @Test func vaultClientSubprocessRejectsMismatchedReadback() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let script = "#!/bin/sh\nif [ \"$3\" = set ]; then cat >/dev/null; exit 0; fi\nprintf '%s' '{\"path\":\"work/item\",\"fields\":{\"password\":\"different\"}}'\n"
        let binary = dir.appendingPathComponent("symvault")
        try script.data(using: .utf8)!.write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let client = VaultClient(userOverride: binary)
        await #expect(throws: CLIRunnerError.self) {
            _ = try await client.set(path: "work/item", field: "password", value: "requested")
        }
    }

}
#endif
