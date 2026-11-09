import Foundation
import SymBrainCore
import SymairaTheme

/// The optional modules Brain manages as separate binaries (PB-2026-09-09 §2).
enum ManagedModule: String, CaseIterable, Identifiable, Hashable {
    case operate
    case scope

    var id: String { rawValue }

    var title: String {
        switch self {
        case .operate: "Operate"
        case .scope: "Scope"
        }
    }

    var binary: String {
        switch self {
        case .operate: "symoperate"
        case .scope: "symscope"
        }
    }

    var summary: String {
        switch self {
        case .operate: "Screen automation and the permissions it needs"
        case .scope: "Inventory of ports, MCP servers, containers and daemons"
        }
    }

    var icon: SymairaIcon {
        switch self {
        case .operate: .window
        case .scope: .network
        }
    }
}

/// What the module sheet needs from either module's view model.
@MainActor
protocol ManagedModuleModel: ObservableObject {
    var moduleEnabled: Bool { get }
    var availability: RuntimeAvailability { get }
    var binaryPath: String? { get }
    var provenance: ManagedBinaryProvenance? { get }
    var profileExposure: [ModuleProfileExposure] { get }
    var isLoading: Bool { get }
    var errorMessage: String? { get }
    var errorDetail: String? { get }
    var isBuilding: Bool { get }
    var sourceRoot: URL? { get }
    var setupCommand: String { get }
    var enableCommand: String { get }
    func refresh() async
    func selectSourceRoot(_ selectedURL: URL)
    func buildAndInstall(sourceRoot: URL) async
}

extension OperateViewModel: ManagedModuleModel {}
extension ScopeViewModel: ManagedModuleModel {}
