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

    @Test func reportGenericErrorUsesSafeMessageWithoutDetail() async {
        let vm = MemoryViewModel()
        let error = NSError(domain: "test", code: 42, userInfo: [
            NSLocalizedDescriptionKey: "something broke at /Users/daniel/private/token.json"
        ])

        vm.report(error)

        #expect(vm.errorMessage == "Something went wrong. Please try again or check the CLI logs for details.")
        #expect(vm.errorDetail == nil)
        #expect(!vm.errorMessage!.contains("/Users/"))
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

    @Test func reportExecutionFailedSetsSafeMessageWithoutDetail() async {
        let vm = SkillsViewModel()
        let error = CLIRunnerError.executionFailed(code: 1, fullStderr: "bad input token=super-secret-token")

        vm.report(error)

        #expect(vm.errorMessage != nil)
        #expect(vm.errorDetail == nil)
        #expect(!vm.errorMessage!.contains("super-secret-token"))
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
    func generatePassword(length: Int, symbols: Bool, profile: String?) async throws -> String { "generated-fixture" }
    func intakePreview(files: [URL], profile: String?) async throws -> VaultIntakeResponse { VaultIntakeResponse(importID: nil, results: []) }
    func intakeStage(files: [URL], ocrTexts: [URL: URL], moveToTrash: Bool, profile: String?) async throws -> VaultIntakeResponse { VaultIntakeResponse(importID: "fixture", results: []) }
    func intakeReviewBatches(profile: String?) async throws -> [String] { ["fixture"] }
    func intakePromote(importID: String, overwrite: Bool, profile: String?) async throws {}
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
    func generatePassword(length: Int, symbols: Bool, profile: String?) async throws -> String { "generated-fixture" }
    func intakePreview(files: [URL], profile: String?) async throws -> VaultIntakeResponse { VaultIntakeResponse(importID: nil, results: []) }
    func intakeStage(files: [URL], ocrTexts: [URL: URL], moveToTrash: Bool, profile: String?) async throws -> VaultIntakeResponse { VaultIntakeResponse(importID: "fixture", results: []) }
    func intakeReviewBatches(profile: String?) async throws -> [String] { ["fixture"] }
    func intakePromote(importID: String, overwrite: Bool, profile: String?) async throws {}

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
    @Test func createEntryClearsCompleteDraftAfterSuccessfulCreate() async {
        let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "x", modified: nil, fields: [:])))
        vm.availability = .ready
        vm.createPath = "work/new"
        vm.createValue = "do-not-retain"
        vm.createType = "basic_auth"
        vm.createUsername = "alice"
        vm.createURL = "https://example.invalid"
        vm.createNotes = "fixture note"
        vm.createUsageHint = "fixture hint"
        vm.createAutoRotate = true
        vm.createExpiresAt = "2030-01-01T00:00:00Z"
        vm.createTOTPSecret = "totp-fixture"
        vm.createTOTPIssuer = "Example"
        vm.createTOTPAccount = "alice@example.invalid"

        await vm.createEntry()

        #expect(vm.createPath.isEmpty)
        #expect(vm.createValue.isEmpty)
        #expect(vm.createType == "password")
        #expect(vm.createUsername.isEmpty)
        #expect(vm.createURL.isEmpty)
        #expect(vm.createNotes.isEmpty)
        #expect(vm.createUsageHint.isEmpty)
        #expect(vm.createAutoRotate == false)
        #expect(vm.createExpiresAt.isEmpty)
        #expect(vm.createTOTPSecret.isEmpty)
        #expect(vm.createTOTPIssuer.isEmpty)
        #expect(vm.createTOTPAccount.isEmpty)
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
        for field in ["private_key", "recovery_key", "client_key"] {
            await #expect(throws: CLIRunnerError.self) {
                _ = try await client.set(path: "work/item", field: field, value: "line-one\nline-two")
            }
        }
        #expect(!FileManager.default.fileExists(atPath: marker.path))
    }

    @Test func vaultClientRejectsEmptyGenericSensitiveKeysBeforeDispatch() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let marker = dir.appendingPathComponent("invoked")
        let script = "#!/bin/sh\ntouch \"\(marker.path)\"\n"
        let binary = dir.appendingPathComponent("symvault")
        try Data(script.utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let client = VaultClient(userOverride: binary)

        for field in ["recovery_key", "client_key"] {
            do {
                _ = try await client.set(path: "work/item", field: field, value: "")
                Issue.record("accepted an empty generic sensitive field")
            } catch let error as CLIRunnerError {
                guard case .invalidJSON(let description) = error else {
                    Issue.record("empty generic key returned the wrong error: \(error)")
                    continue
                }
                #expect(description == "sensitive field values cannot be empty")
            } catch {
                Issue.record("empty generic key returned a non-CLI error: \(error)")
            }
            #expect(!FileManager.default.fileExists(atPath: marker.path))
        }
    }

    @Test func vaultClientRejectsCRLFCRAndLFBeforeDispatchForCreateAndSet() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let marker = dir.appendingPathComponent("invoked")
        let script = "#!/bin/sh\ntouch \"\(marker.path)\"\ncase \"$3\" in add|set) cat >/dev/null; exit 0;; get) printf '%s' '{\"path\":\"work/item\",\"fields\":{\"password\":\"control\"}}'; exit 0;; esac\nexit 1\n"
        let binary = dir.appendingPathComponent("symvault")
        try script.data(using: .utf8)!.write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let client = VaultClient(userOverride: binary)

        // Mutation-negative control: this fixture reliably marks a dispatched command.
        _ = try await client.create(path: "work/item", value: "control")
        #expect(FileManager.default.fileExists(atPath: marker.path))
        try FileManager.default.removeItem(at: marker)
        _ = try await client.set(path: "work/item", field: "password", value: "control")
        #expect(FileManager.default.fileExists(atPath: marker.path))
        try FileManager.default.removeItem(at: marker)

        for value in ["line-one\r\nline-two", "line-one\rline-two", "line-one\nline-two"] {
            do {
                _ = try await client.create(path: "work/item", value: value)
                Issue.record("create accepted a multiline secret")
            } catch let error as CLIRunnerError {
                if case .invalidJSON(let description) = error {
                    #expect(description == "multiline secret values are not supported")
                } else {
                    Issue.record("create returned the wrong CLIRunnerError: \(error)")
                }
            } catch {
                Issue.record("create returned a non-CLI error: \(error)")
            }
            #expect(!FileManager.default.fileExists(atPath: marker.path))

            do {
                _ = try await client.set(path: "work/item", field: "password", value: value)
                Issue.record("set accepted a multiline secret")
            } catch let error as CLIRunnerError {
                if case .invalidJSON(let description) = error {
                    #expect(description == "multiline secret values are not supported")
                } else {
                    Issue.record("set returned the wrong CLIRunnerError: \(error)")
                }
            } catch {
                Issue.record("set returned a non-CLI error: \(error)")
            }
            #expect(!FileManager.default.fileExists(atPath: marker.path))
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

#if os(macOS)

@Test func vaultClientUpdateCanClearMetadataWithoutTouchingSecret() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let argsFile = dir.appendingPathComponent("args")
    let script = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"\(argsFile.path)\"\nif [ \"$3\" = set ]; then cat >/dev/null; exit 0; fi\nprintf '%s' '{\"path\":\"work/item\",\"type\":\"database_url\",\"fields\":{\"connection_string\":\"db-secret\",\"username\":\"\",\"url\":\"https://example.invalid\",\"notes\":\"new note\"}}'\n"
    let binary = dir.appendingPathComponent("symvault")
    try Data(script.utf8).write(to: binary)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
    let client = VaultClient(userOverride: binary)
    let original = VaultEntryDetail(path: "work/item", modified: nil, fields: [
        "connection_string": .string("db-secret"), "username": .string("alice"),
        "url": .string("https://example.invalid"), "notes": .string("old note")
    ], type: "database_url", usageHint: "keep", autoRotate: true, expiresAt: "2030-01-01T00:00:00Z")
    let confirmation = try await client.update(path: original.path, original: original, draft: VaultCredentialDraft(path: original.path, type: "database_url", username: "", url: "https://example.invalid", notes: "new note"))
    #expect(confirmation.confirmedValueMatches)
    let args = try String(contentsOf: argsFile)
    #expect(args.contains("work/item.username"))
    #expect(args.contains("work/item.notes"))
    #expect(!args.contains("work/item.connection_string"))
}

@Test func vaultClientGenerationAndIntakeUseExactCLIContracts() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let argsFile = dir.appendingPathComponent("args")
    let script = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"\(argsFile.path)\"\ncase \"$3\" in generate) printf 'fixture-password\\n';; intake) printf '%s' '{\"results\":[]}' ;; esac\n"
    let binary = dir.appendingPathComponent("symvault")
    try Data(script.utf8).write(to: binary)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
    let client = VaultClient(userOverride: binary)

    let generated = try await client.generatePassword(length: 32, symbols: true)
    #expect(generated == "fixture-password")
    let generationArgs = try String(contentsOf: argsFile).split(separator: "\n").map(String.init)
    #expect(generationArgs == ["--color", "never", "generate", "--length", "32", "--symbols"])

    _ = try await client.intakePreview(files: [URL(fileURLWithPath: "/tmp/credentials.env")])
    let intakeArgs = try String(contentsOf: argsFile).split(separator: "\n").map(String.init)
    #expect(intakeArgs == ["--color", "never", "intake", "/tmp/credentials.env", "--dry-run", "--json"])
}

@MainActor
@Test func generatedPasswordIsClearedAfterViewModelOperation() async {
    let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "x", modified: nil, fields: [:])))
    await vm.generatePassword(length: 24, symbols: true)
    #expect(vm.generatedPassword.isEmpty)
    #expect(vm.createValue == "generated-fixture")
}

@Test func vaultClientRejectsPasswordLengthAboveServiceMaximumBeforeDispatch() async {
    let client = VaultClient(userOverride: URL(fileURLWithPath: "/tmp/not-a-real-symvault"))
    do {
        _ = try await client.generatePassword(length: 1025, symbols: false)
        Issue.record("accepted password length above symvault MaxPasswordLength")
    } catch let error as CLIRunnerError {
        guard case .invalidJSON(let description) = error else {
            Issue.record("wrong error for over-limit generation: \(error)")
            return
        }
        #expect(description == "password length must be between 1 and 1024")
    } catch {
        Issue.record("wrong error type for over-limit generation: \(error)")
    }
}

@Test func vaultClientRejectsMultipleOCRInputsBeforeDispatch() async {
    let client = VaultClient(userOverride: URL(fileURLWithPath: "/tmp/not-a-real-symvault"))
    let first = URL(fileURLWithPath: "/tmp/ocr-one.txt")
    let second = URL(fileURLWithPath: "/tmp/ocr-two.txt")
    do {
        _ = try await client.intakeStage(files: [URL(fileURLWithPath: "/tmp/source.png")], ocrTexts: [first: first, second: second])
        Issue.record("accepted multiple OCR inputs")
    } catch let error as CLIRunnerError {
        guard case .invalidJSON(let description) = error else {
            Issue.record("wrong error for multiple OCR inputs: \(error)")
            return
        }
        #expect(description == "symvault intake accepts one --ocr-text file per invocation")
    } catch {
        Issue.record("wrong error type for multiple OCR inputs: \(error)")
    }
}


@Test func vaultClientCreateUsesMetadataAndOrderedTOTPStdinAndRejectsPaymentCreation() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let argsFile = dir.appendingPathComponent("args")
    let stdinFile = dir.appendingPathComponent("stdin")
    let marker = dir.appendingPathComponent("payment-invoked")
    let script = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"\(argsFile.path)\"\nif [ \"$3\" = add ]; then cat > \"\(stdinFile.path)\"; exit 0; fi\nif [ \"$3\" = get ]; then printf '%s' '{\"path\":\"work/basic\",\"type\":\"basic_auth\",\"fields\":{\"basic_auth\":\"redacted\",\"username\":\"alice\"}}'; exit 0; fi\ntouch \"\(marker.path)\"; exit 1\n"
    let binary = dir.appendingPathComponent("symvault")
    try Data(script.utf8).write(to: binary)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
    let client = VaultClient(userOverride: binary)
    let secret = String(repeating: "s", count: 19)
    let totp = String(repeating: "t", count: 16)

    let confirmation = try await client.create(draft: VaultCredentialDraft(
        path: "work/basic", type: "basic_auth", secret: secret, username: "alice",
        url: "https://example.invalid", notes: "fixture notes", usageHint: "fixture hint",
        autoRotate: true, expiresAt: "2030-01-01T00:00:00Z", totpSecret: totp,
        totpIssuer: "Example", totpAccount: "alice@example.invalid"))
    #expect(confirmation.confirmedPath == "work/basic")

    let args = try String(contentsOf: argsFile).split(separator: "\n").map(String.init)
    #expect(Array(args.prefix(23)) == [
        "--color", "never", "add", "work/basic", "--stdin-value", "--type", "basic_auth",
        "--username", "alice", "--url", "https://example.invalid", "--notes", "fixture notes",
        "--usage-hint", "fixture hint", "--expires-at", "2030-01-01T00:00:00Z", "--auto-rotate",
        "--stdin-totp-secret", "--totp-issuer", "Example", "--totp-account", "alice@example.invalid"
    ])
    let stdin = try Data(contentsOf: stdinFile)
    let records = stdin.split(separator: 0x0A, omittingEmptySubsequences: false)
    #expect(records.count == 3)
    #expect(records[0].count == 19)
    #expect(records[1].count == 16)
    #expect(records[2].isEmpty)

    do {
        _ = try await client.create(draft: VaultCredentialDraft(path: "work/payment", type: "payment", secret: secret))
        Issue.record("payment creation was accepted")
    } catch let error as CLIRunnerError {
        guard case .invalidJSON(let description) = error else {
            Issue.record("payment creation returned the wrong error: \(error)")
            return
        }
        #expect(description == "invalid credential path or type")
    } catch {
        Issue.record("payment creation returned a non-CLI error: \(error)")
    }
    #expect(!FileManager.default.fileExists(atPath: marker.path))
}

@MainActor
@Test func approvalReviewBindsSnapshotAndRejectsStaleOrCancelledSelection() async {
    let vm = VaultViewModel(client: StubVaultClient(result: VaultEntryDetail(path: "x", modified: nil, fields: [:])))
    vm.availability = .ready
    let request = VaultApprovalRequest(id: "apr-1", agentName: "agent", path: "work/file", write: true, reason: "needs approval", createdAt: "2030-01-01T00:00:00Z", expiresAt: "2099-01-01T00:00:00Z", status: "pending")
    vm.approvalRequests = [request]
    vm.reviewApproval(request)
    #expect(vm.approvalSnapshot?.request.id == "apr-1")
    vm.cancelApprovalReview()
    #expect(vm.approvalSnapshot == nil)
    let expired = VaultApprovalRequest(id: "apr-2", agentName: "agent", path: "work/expired", write: true, reason: "expired", createdAt: "2030-01-01T00:00:00Z", expiresAt: "2000-01-01T00:00:00Z", status: "pending")
    vm.approvalRequests = [expired]
    vm.reviewApproval(expired)
    #expect(vm.approvalSnapshot == nil)
    #expect(vm.errorMessage == "That approval request is stale or expired.")
}

#endif
