// ModuleViewModelBase — shared skeleton for module view models.
//
// Every module view model (Memory, Vault, Skills) repeats the same
// isLoading / errorMessage / errorDetail / isBinaryNotFound /
// statusMessage bundle plus clearError() and report(_:) logic.
// This protocol extracts that into a single definition with default
// implementations so new modules get the mapping for free and existing
// modules stay in sync.

#if os(macOS)
import Foundation
import SymairaCLIRunner

/// Common requirements for a module view model.
///
/// Adopters inherit default implementations of `clearError()` and
/// `report(_:)` via the protocol extension below.  Conformers must
/// supply their own `@Published` storage for each requirement — the
/// protocol extension cannot declare stored properties.
@MainActor
public protocol ModuleViewModelProtocol: ObservableObject {
    var isLoading: Bool { get set }
    var errorMessage: String? { get set }
    var errorDetail: String? { get set }
    var statusMessage: String? { get set }
    var isBinaryNotFound: Bool { get set }

    func clearError()
    func report(_ error: Error)
}

// MARK: - Default implementations

extension ModuleViewModelProtocol {

    /// Resets all error-related state to its initial value.
    public func clearError() {
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
    }

    /// Maps *error* into `errorMessage` / `errorDetail` and flags
    /// `isBinaryNotFound` when the underlying CLI binary is missing.
    public func report(_ error: Error) {
        let friendly = formatError(error)
        errorMessage = friendly.message
        errorDetail = friendly.detail
        if let cliError = error as? CLIRunnerError,
           case .binaryNotFound = cliError {
            isBinaryNotFound = true
        }
    }
}
#endif
