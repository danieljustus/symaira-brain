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
