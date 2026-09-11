#if os(macOS)
import Foundation
import Testing
@testable import SymBrainCore

@MainActor
struct VaultIntakeStateMachineTests {
    @Test func previewAndMissingImportIDNeverEnablePromotion() async {
        let source = URL(fileURLWithPath: "/tmp/vault-intake-preview.env")
        let accepted = intakeResult(file: source.path, status: "ok")
        let rejected = intakeResult(file: "/tmp/not-a-credential.txt", status: "rejected")
        let client = IntakeSpyVaultClient(
            previewResponse: VaultIntakeResponse(importID: nil, results: [accepted, rejected]),
            stageResponse: VaultIntakeResponse(importID: nil, results: [accepted, rejected])
        )
        let vm = VaultViewModel(client: client)

        vm.setIntakeFiles([source])
        await vm.previewIntake()

        #expect(client.previewRequests == [[source]])
        #expect(vm.intakePhase == .reviewing)
        #expect(vm.intakePreview?.results == [accepted, rejected])
        #expect(vm.intakeDrafts.map(\.file) == [accepted.file])
        #expect(vm.intakeImportID == nil)
        #expect(vm.intakeReviewComplete == false)

        await vm.stageIntake()
        vm.markIntakeReviewed()
        await vm.promoteIntake()

        #expect(client.stageRequests == [[source]])
        #expect(vm.intakePhase == .failed("Vault service did not return an import ID."))
        #expect(vm.intakeImportID == nil)
        #expect(vm.intakeReviewComplete == false)
        #expect(vm.intakeReviewSnapshot == nil)
        #expect(client.promotedImportIDs.isEmpty)
    }

    @Test func promotionRequiresAnUnchangedReviewedBatch() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let source = directory.appendingPathComponent("account.env")
        try Data("initial fixture".utf8).write(to: source)
        let accepted = intakeResult(file: source.path, status: "ok")
        let client = IntakeSpyVaultClient(
            previewResponse: VaultIntakeResponse(importID: nil, results: []),
            stageResponse: VaultIntakeResponse(importID: "batch-42", results: [accepted])
        )
        let vm = VaultViewModel(client: client)

        vm.setIntakeFiles([source])
        await vm.stageIntake()

        #expect(vm.intakePhase == .reviewing)
        #expect(vm.intakeImportID == "batch-42")
        #expect(vm.intakeDrafts.map(\.file) == [accepted.file])

        await vm.promoteIntake()
        #expect(vm.errorMessage == "Review the staged intake before promoting it.")
        #expect(client.promotedImportIDs.isEmpty)

        vm.markIntakeReviewed()
        #expect(vm.intakeReviewComplete)
        #expect(vm.intakeReviewSnapshot?.importID == "batch-42")

        vm.intakeDrafts[0].targetPath = "reviewed/account"
        await vm.promoteIntake()

        #expect(vm.errorMessage == "The staged intake changed; review it again before promoting.")
        #expect(vm.intakeReviewComplete == false)
        #expect(vm.intakeReviewSnapshot == nil)
        #expect(client.promotedImportIDs.isEmpty)

        vm.markIntakeReviewed()
        try Data("changed fixture".utf8).write(to: source)
        await vm.promoteIntake()

        #expect(vm.errorMessage == "The staged intake changed; review it again before promoting.")
        #expect(vm.intakeReviewComplete == false)
        #expect(vm.intakeReviewSnapshot == nil)
        #expect(client.promotedImportIDs.isEmpty)

        vm.markIntakeReviewed()
        await vm.promoteIntake()

        #expect(client.promotedImportIDs == ["batch-42"])
        #expect(client.promotedOverwrites == [false])
        #expect(vm.intakePhase == .done)
        #expect(vm.statusMessage == "Intake batch promoted and confirmed by the vault service.")
    }
}

private func intakeResult(file: String, status: String) -> VaultIntakeFileResult {
    VaultIntakeFileResult(
        file: file,
        status: status,
        reason: status == "ok" ? nil : "fixture rejection",
        provenance: nil,
        suggestions: [],
        duplicates: []
    )
}

private enum IntakeSpyError: Error {
    case unexpectedCall
}

@MainActor
private final class IntakeSpyVaultClient: VaultClientProtocol {
    var isInstalled: Bool { true }
    let previewResponse: VaultIntakeResponse
    let stageResponse: VaultIntakeResponse
    private(set) var previewRequests: [[URL]] = []
    private(set) var stageRequests: [[URL]] = []
    private(set) var promotedImportIDs: [String] = []
    private(set) var promotedOverwrites: [Bool] = []

    init(previewResponse: VaultIntakeResponse, stageResponse: VaultIntakeResponse) {
        self.previewResponse = previewResponse
        self.stageResponse = stageResponse
    }

    func availability(profile: String?) async -> VaultAvailability { .ready }
    func version(profile: String?) async throws -> String { "fixture" }
    func unlock(passphrase: String, ttl: String, profile: String?) async throws {}
    func lock(profile: String?) async throws {}
    func list(profile: String?) async throws -> [VaultEntrySummary] { [] }
    func find(query: String, profile: String?) async throws -> [VaultEntrySummary] { [] }
    func entry(path: String, profile: String?) async throws -> VaultEntryDetail { throw IntakeSpyError.unexpectedCall }
    func create(path: String, value: String, profile: String?) async throws -> VaultCreateConfirmation { throw IntakeSpyError.unexpectedCall }
    func set(path: String, field: String, value: String, profile: String?) async throws -> VaultSetConfirmation { throw IntakeSpyError.unexpectedCall }
    func delete(path: String, profile: String?) async throws -> VaultDeleteConfirmation { throw IntakeSpyError.unexpectedCall }
    func generatePassword(length: Int, symbols: Bool, profile: String?) async throws -> String { throw IntakeSpyError.unexpectedCall }

    func intakePreview(files: [URL], profile: String?) async throws -> VaultIntakeResponse {
        previewRequests.append(files)
        return previewResponse
    }

    func intakeStage(
        files: [URL],
        ocrTexts: [URL: URL],
        moveToTrash: Bool,
        profile: String?
    ) async throws -> VaultIntakeResponse {
        stageRequests.append(files)
        return stageResponse
    }

    func intakeReviewBatches(profile: String?) async throws -> [String] { [] }

    func intakePromote(importID: String, overwrite: Bool, profile: String?) async throws {
        promotedImportIDs.append(importID)
        promotedOverwrites.append(overwrite)
    }
}
#endif
