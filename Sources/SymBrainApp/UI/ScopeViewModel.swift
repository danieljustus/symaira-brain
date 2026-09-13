// ScopeViewModel — state behind the embedded Symaira Scope module.
//
// Scope (local system inventory: listening ports, MCP server configs,
// containers, launchd/Homebrew daemons) is an optional module (contract
// PB-2026-09-09 §2): `[modules] scope` in symbrain's global config gates
// whether `symbrain setup`/`doctor --fix` manage its binary, and
// `internal/profile`'s ServerScope ("scope") gates whether a given profile
// exposes it as an MCP tool.
//
// Scope ships as its own managed binary (symscope), the same shape as Vault
// and Operate, so this mirrors OperateViewModel/VaultView's CLI-shell-out
// pattern rather than reimplementing Cockpit's live port/daemon/container
// dashboard in-process. `symscope scan` walks lsof, launchctl and docker —
// real work, not a status probe — so it stays behind an explicit "Scan Now"
// button, and its result is reduced to counts rather than the full item
// tables Cockpit renders (kept out of scope on purpose: "basic status/
// health", not a full feature reinvention).

#if os(macOS)
import Foundation
import SymBrainCore
import SymairaCLIRunner
import SymairaToolKit

// MARK: - symscope JSON models (scope/Sources/SymScopeCore/Models.swift)

/// Reduced result of `symscope scan`: counts only. Each per-item shape
/// (`Port`, `Container`, `Daemon`, `MCPServer`) is intentionally not mirrored
/// here — this view answers "how much is out there", not "browse it",
/// avoiding tight coupling to scope's evolving per-item schemas.
struct ScopeSnapshotCounts: Decodable, Sendable, Equatable {
    struct EmptyElement: Decodable, Sendable, Equatable {}

    let generatedAt: String
    let notes: [String]
    let ports: [EmptyElement]
    let mcpServers: [EmptyElement]
    let containers: [EmptyElement]
    let daemons: [EmptyElement]

    var portCount: Int { ports.count }
    var mcpServerCount: Int { mcpServers.count }
    var containerCount: Int { containers.count }
    var daemonCount: Int { daemons.count }
}

// MARK: - ScopeClient

/// Locates and executes the `symscope` managed binary. Mirrors
/// `OperateClient`/`VaultClient`: Scope ships as its own process, so binary
/// resolution is self-contained.
struct ScopeClient: Sendable {
    static let binaryName = "symscope"

    let locator: BinaryLocator
    let runner: CLIRunner

    init(
        userOverride: URL? = nil,
        searchPATH: String? = nil,
        extraDirectories: [String] = [
            "~/.symaira/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
        ],
        runner: CLIRunner = CLIRunner()
    ) {
        self.runner = runner
        self.locator = BinaryLocator(
            bundle: nil,
            userOverride: userOverride,
            searchPATH: searchPATH,
            extraDirectories: extraDirectories
        )
    }

    func resolveBinary() -> URL? {
        locator.locate(Self.binaryName, allowUnverified: true)?.url
    }

    var isInstalled: Bool { resolveBinary() != nil }

    private func executable() throws -> URL {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: Self.binaryName)
        }
        return binary
    }

    /// `symscope version --json` — local only, no network or filesystem scan.
    /// Reuses `SymBrainCore.VersionInfo`: symscope's version payload
    /// (`tool`/`version`/`schema_version`) matches symbrain's own contract.
    func version() async throws -> VersionInfo {
        let result = try await runner.runAllowingFailure(try executable(), arguments: ["version", "--json"], timeout: 10)
        return try ManagedModuleSupport.decodeSnake(VersionInfo.self, stdout: result.stdoutText, stderr: result.stderrText)
    }

    /// `symscope scan` — walks lsof/launchctl/docker; real work, not a cheap probe.
    func scan(timeout: Double = 30) async throws -> ScopeSnapshotCounts {
        let result = try await runner.runAllowingFailure(try executable(), arguments: ["scan"], timeout: timeout)
        return try ManagedModuleSupport.decodeSnake(ScopeSnapshotCounts.self, stdout: result.stdoutText, stderr: result.stderrText)
    }
}

// MARK: - ScopeViewModel

@MainActor
final class ScopeViewModel: ObservableObject, ModuleViewModelProtocol {
    @Published var isLoading = false
    @Published var errorMessage: String?
    @Published var errorDetail: String?
    @Published var isBinaryNotFound = false
    @Published var statusMessage: String?

    /// `[modules] scope` in symbrain's global config.
    @Published private(set) var moduleEnabled = false
    @Published private(set) var availability: RuntimeAvailability = .checking
    @Published private(set) var binaryPath: String?
    @Published private(set) var provenance: ManagedBinaryProvenance?
    @Published private(set) var profileExposure: [ModuleProfileExposure] = []
    @Published var brokerActivity: [AuditEntry] = []

    @Published var versionInfo: VersionInfo?
    @Published var isCheckingVersion = false

    @Published var snapshot: ScopeSnapshotCounts?
    @Published var isScanning = false

    @Published var isBuilding = false
    @Published var buildResult: SetupSourceResult?
    /// Selected only for this view-model session. Never inferred or persisted.
    @Published private(set) var sourceRoot: URL?

    private let client: ScopeClient
    private let symbrain: SymBrainClient
    private let auditReader = AuditLogReader()

    init(symbrainClient: SymBrainClient, client: ScopeClient = ScopeClient()) {
        self.symbrain = symbrainClient
        self.client = client
    }

    var setupCommand: String {
        ModuleSourceBuild.copyableSetupCommand(module: "scope", sourceRoot: sourceRoot)
    }
    var enableCommand: String { "symbrain config set modules.scope true" }

    /// Loads everything the Scope screen shows on its own, without running
    /// the lsof/launchctl/docker inventory scan.
    func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        moduleEnabled = (try? await ManagedModuleSupport.isEnabled(module: "scope", symbrain: symbrain)) ?? false

        if let binary = client.resolveBinary() {
            binaryPath = binary.path
            provenance = ManagedModuleSupport.readProvenance(nextTo: binary)
            availability = .ready
            isBinaryNotFound = false
        } else {
            binaryPath = nil
            provenance = nil
            availability = .missing
            isBinaryNotFound = true
        }

        await loadProfileExposure()
        await loadActivity()
    }

    func loadProfileExposure() async {
        profileExposure = await ManagedModuleSupport.profileExposure(server: "scope", symbrain: symbrain)
    }

    /// Reads the symbrain broker audit log and keeps the scope server's calls.
    func loadActivity() async {
        brokerActivity = await auditReader.read(profile: nil, server: "scope", limit: 500)
    }

    /// Stores a canonical, structurally valid checkout only for this session.
    /// The UI supplies this from an explicit open panel; no source root is
    /// inferred from the app bundle, current directory, PATH, or source file.
    func selectSourceRoot(_ selectedURL: URL) {
        do {
            sourceRoot = try ModuleSourceBuild.canonicalSourceRoot(validating: selectedURL)
            errorMessage = nil
            errorDetail = nil
        } catch let error as ModuleSourceBuild.ValidationError {
            sourceRoot = nil
            errorMessage = error.localizedDescription
            errorDetail = Self.sourceCheckoutGuidance
        } catch {
            sourceRoot = nil
            errorMessage = "Could not validate the selected source checkout."
            errorDetail = Self.sourceCheckoutGuidance
        }
    }

    func checkVersion() async {
        isCheckingVersion = true
        defer { isCheckingVersion = false }
        do {
            versionInfo = try await client.version()
        } catch {
            report(error)
        }
    }

    /// Runs `symscope scan` — a real lsof/launchctl/docker inventory pass,
    /// reduced to counts for display.
    func runScan() async {
        isScanning = true
        clearError()
        defer { isScanning = false }
        do {
            snapshot = try await client.scan()
        } catch {
            report(error)
        }
    }

    /// Builds symscope from an explicitly selected in-repo source checkout
    /// and installs it, then reloads status. See OperateViewModel
    /// buildAndInstall for why this still requires the module to be enabled
    /// in config.
    func buildAndInstall(sourceRoot: URL) async {
        guard moduleEnabled else { return }
        isBuilding = true
        clearError()
        defer { isBuilding = false }
        do {
            let canonicalSourceRoot = try ModuleSourceBuild.canonicalSourceRoot(validating: sourceRoot)
            self.sourceRoot = canonicalSourceRoot
            let outcome = try await ManagedModuleSupport.setupFromSource(
                module: "scope",
                sourceRoot: canonicalSourceRoot,
                symbrain: symbrain
            )
            buildResult = outcome.firstResult
            if let failure = outcome.firstResult, failure.status == "error" {
                errorMessage = failure.error ?? "Build failed."
            }
            await refresh()
        } catch let error as ModuleSourceBuild.ValidationError {
            self.sourceRoot = nil
            errorMessage = error.localizedDescription
            errorDetail = Self.sourceCheckoutGuidance
        } catch {
            report(error)
        }
    }

    private static let sourceCheckoutGuidance = "Choose the Symaira Brain checkout root containing go.mod, browse/, operate/Package.swift, scope/Package.swift, and history/Package.swift."
}
#endif
