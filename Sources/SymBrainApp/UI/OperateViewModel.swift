// OperateViewModel — state behind the embedded Symaira Operate module.
//
// Operate (screen automation: apps/windows/displays, clicks, typing, OCR) is
// an optional module (contract PB-2026-09-09 §2): `[modules] operate` in
// symbrain's global config gates whether `symbrain setup`/`doctor --fix`
// manage its binary at all, and `internal/profile`'s ServerOperate ("operate")
// gates whether a given profile exposes it as an MCP tool.
//
// Unlike Memory (embedded in the symbrain binary itself), Operate ships as
// its own managed binary (symoperate) — the same shape as Vault — so this
// view model mirrors VaultView's pattern (VaultClient + BinaryLocator +
// CLIRunner) rather than MemoryClient's "brain:"-threaded one. It does not
// perform automation itself: it only answers "is this module set up, and
// what does it report about itself" — clicking and typing stay in the CLI
// and the MCP server, where an agent's action policy applies.
//
// `symoperate doctor` takes a real screenshot and queries the frontmost app's
// accessibility tree; `symoperate version` makes an outbound GitHub release
// check. Both are useful "basic status/health" signals the CLI already
// exposes, but neither is side-effect-free, so both stay behind explicit
// buttons rather than running automatically on `refresh()`.

#if os(macOS)
import Foundation
import SymBrainCore
import SymairaCLIRunner
import SymairaToolKit

// MARK: - symoperate JSON models (operate/Sources/SymOperateCore/Models.swift, DoctorReport.swift)

struct OperatePermissionSource: Decodable, Sendable, Equatable {
    let pid: Int32
    let ppid: Int32
    let executablePath: String
    let launchingProcessName: String?
    let note: String
}

struct OperatePermissionSnapshot: Decodable, Sendable, Equatable {
    let accessibilityGranted: Bool
    let screenRecordingGranted: Bool
    let source: OperatePermissionSource
}

struct OperateEnvironmentReport: Decodable, Sendable, Equatable {
    let platform: String
    let macOSVersion: String
    let swiftVersion: String
    let appsCount: Int
    let displaysCount: Int
}

/// Result of `symoperate doctor`. Its exit code reflects whether every probe
/// passed, but the JSON is printed either way, so this is decoded from
/// stdout directly rather than gated on a zero exit.
struct OperateDoctorReport: Decodable, Sendable, Equatable {
    let ok: Bool
    let version: String
    let permissions: OperatePermissionSnapshot
    let capabilities: [String: Bool]
    let environment: OperateEnvironmentReport
    let recommendations: [String]
    let effectiveGrant: [String]
}

/// Result of `symoperate version`. Performs a live GitHub release check.
struct OperateVersionReport: Decodable, Sendable, Equatable {
    let version: String
    let updateAvailable: Bool
    let latestVersion: String?
    let releaseURL: String?
    let error: String?
}

// MARK: - Shared managed-module support (also used by ScopeViewModel)

/// A managed binary's origin sidecar, written by `symbrain setup
/// --from-source` / the release installer next to the binary in
/// `~/.symaira/bin` (see internal/managed/provenance.go). Read directly from
/// disk: there is no CLI surface for it yet (`symbrain doctor --json` only
/// tracks `vault` in its `servers` list and the release manifest's cores,
/// neither of which includes brain-source-built operate/scope binaries).
struct ManagedBinaryProvenance: Decodable, Sendable, Equatable {
    let binary: String
    let source: String
    let version: String
    let repo: String?
    let receiverCommit: String?
    let moduleDir: String?
    let builder: String?
    let builtAt: String?
    let binarySHA256: String

    enum CodingKeys: String, CodingKey {
        case binary, source, version, repo
        case receiverCommit = "receiver_commit"
        case moduleDir = "module_dir"
        case builder
        case builtAt = "built_at"
        case binarySHA256 = "binary_sha256"
    }

    var isBrainSource: Bool { source == "brain-source" }
    var formattedBuiltAt: String? { builtAt.map(formatModuleTimestamp) }
}

/// Whether one profile exposes a module's server, from `symbrain profile
/// list --json` (`internal/profile`'s ServerOperate/ServerScope keys).
struct ModuleProfileExposure: Identifiable, Sendable, Equatable {
    let profile: String
    let enabled: Bool
    let mode: String?
    var id: String { profile }
}

struct SetupSourceResult: Decodable, Sendable, Equatable {
    let module: String
    let binary: String
    let version: String?
    let status: String
    let source: String?
    let receiverCommit: String?
    let binarySHA256: String?
    let error: String?

    enum CodingKeys: String, CodingKey {
        case module, binary, version, status, source
        case receiverCommit = "receiver_commit"
        case binarySHA256 = "binary_sha256"
        case error
    }
}

struct SetupSourceReport: Decodable, Sendable, Equatable {
    let binDir: String
    let root: String
    let results: [SetupSourceResult]
    let errors: [String]?

    enum CodingKeys: String, CodingKey {
        case binDir = "bin_dir"
        case root, results, errors
    }

    var firstResult: SetupSourceResult? { results.first }
}

/// Shared plumbing for optional modules that ship as their own managed
/// binary and are gated by `[modules]` in symbrain's global config
/// (Operate, Scope). Kept file-scoped here and reused by ScopeViewModel —
/// the same cross-file sharing MemoryView.swift already uses for
/// `ModuleActivityTable`/`ModuleTabStrip`.
enum ManagedModuleSupport {
    /// Reads `symbrain config get modules.<module>`. A non-zero exit means
    /// the key is unset, which resolves to `false` — `ModulesConfig`'s
    /// documented default (internal/config/config.go).
    static func isEnabled(module: String, symbrain: SymBrainClient) async throws -> Bool {
        guard let binary = symbrain.resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        let result = try await symbrain.runner.run(
            binary,
            arguments: ["config", "get", "modules.\(module)"],
            timeout: 10
        )
        guard result.exitCode == 0 else { return false }
        return result.stdoutText.trimmingCharacters(in: .whitespacesAndNewlines) == "true"
    }

    static func readProvenance(nextTo binary: URL) -> ManagedBinaryProvenance? {
        let sidecar = binary.deletingLastPathComponent()
            .appendingPathComponent(binary.lastPathComponent + ".provenance.json")
        guard let data = try? Data(contentsOf: sidecar) else { return nil }
        return try? JSONDecoder().decode(ManagedBinaryProvenance.self, from: data)
    }

    static func profileExposure(server: String, symbrain: SymBrainClient) async -> [ModuleProfileExposure] {
        guard let profiles = try? await symbrain.profileList() else { return [] }
        return profiles.map { summary in
            let ref = summary.servers.first { $0.server == server }
            return ModuleProfileExposure(profile: summary.name, enabled: ref?.enabled ?? false, mode: ref?.mode)
        }
    }

    /// Runs `symbrain setup --from-source --modules <module> --json`, which
    /// builds the module from the in-repo receiving copy (browse/operate/
    /// scope) and installs it into the managed directory with a provenance
    /// sidecar. Building a Swift package can take a couple of minutes.
    static func setupFromSource(
        module: String,
        symbrain: SymBrainClient,
        timeout: Double = 300
    ) async throws -> SetupSourceReport {
        guard let binary = symbrain.resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        let result = try await symbrain.runner.runAllowingFailure(
            binary,
            arguments: ["setup", "--from-source", "--modules", module, "--json"],
            timeout: timeout
        )
        return try decodePlain(SetupSourceReport.self, stdout: result.stdoutText, stderr: result.stderrText)
    }

    /// Decodes a CLI's own camelCase JSON (with the odd snake_case field,
    /// e.g. symoperate's `effective_grant`) — safe to run `.convertFromSnakeCase`
    /// over since none of these types declare explicit `CodingKeys`.
    static func decodeSnake<T: Decodable>(_ type: T.Type, stdout: String, stderr: String) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decode(type, stdout: stdout, stderr: stderr, using: decoder)
    }

    /// Decodes a type with its own explicit snake_case `CodingKeys` (the
    /// managed-binary provenance/setup-report shapes) — `.convertFromSnakeCase`
    /// would double-transform those keys and break the match, so this uses a
    /// plain decoder instead.
    static func decodePlain<T: Decodable>(_ type: T.Type, stdout: String, stderr: String) throws -> T {
        try decode(type, stdout: stdout, stderr: stderr, using: JSONDecoder())
    }

    private static func decode<T: Decodable>(
        _ type: T.Type,
        stdout: String,
        stderr: String,
        using decoder: JSONDecoder
    ) throws -> T {
        let trimmed = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let data = trimmed.data(using: .utf8) else {
            throw CLIRunnerError.invalidJSON(description: stderr.isEmpty ? "empty response" : stderr)
        }
        do {
            return try decoder.decode(type, from: data)
        } catch {
            throw CLIRunnerError.invalidJSON(description: String(describing: error))
        }
    }
}

// MARK: - OperateClient

/// Locates and executes the `symoperate` managed binary. Mirrors
/// `VaultClient`'s shape: Operate ships as its own process, not a symbrain
/// subcommand, so binary resolution is self-contained rather than threaded
/// through the shared `SymBrainClient` (see MemoryClient's `brain:` for the
/// embedded-module alternative).
struct OperateClient: Sendable {
    static let binaryName = "symoperate"

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

    /// `symoperate version` — also checks GitHub for a newer release.
    func version() async throws -> OperateVersionReport {
        let result = try await runner.runAllowingFailure(try executable(), arguments: ["version"], timeout: 20)
        return try ManagedModuleSupport.decodeSnake(OperateVersionReport.self, stdout: result.stdoutText, stderr: result.stderrText)
    }

    /// `symoperate doctor` — takes a real screenshot and queries the
    /// frontmost app's accessibility tree to prove those capabilities work.
    func doctor() async throws -> OperateDoctorReport {
        let result = try await runner.runAllowingFailure(try executable(), arguments: ["doctor"], timeout: 20)
        return try ManagedModuleSupport.decodeSnake(OperateDoctorReport.self, stdout: result.stdoutText, stderr: result.stderrText)
    }
}

// MARK: - OperateViewModel

@MainActor
final class OperateViewModel: ObservableObject, ModuleViewModelProtocol {
    @Published var isLoading = false
    @Published var errorMessage: String?
    @Published var errorDetail: String?
    @Published var isBinaryNotFound = false
    @Published var statusMessage: String?

    /// `[modules] operate` in symbrain's global config — the master switch
    /// for whether Brain manages this binary at all.
    @Published private(set) var moduleEnabled = false
    @Published private(set) var availability: RuntimeAvailability = .checking
    @Published private(set) var binaryPath: String?
    @Published private(set) var provenance: ManagedBinaryProvenance?
    @Published private(set) var profileExposure: [ModuleProfileExposure] = []
    @Published var brokerActivity: [AuditEntry] = []

    @Published var versionReport: OperateVersionReport?
    @Published var isCheckingVersion = false

    @Published var doctorReport: OperateDoctorReport?
    @Published var isRunningDoctor = false

    @Published var isBuilding = false
    @Published var buildResult: SetupSourceResult?

    private let client: OperateClient
    private let symbrain: SymBrainClient
    private let auditReader = AuditLogReader()

    init(symbrainClient: SymBrainClient, client: OperateClient = OperateClient()) {
        self.symbrain = symbrainClient
        self.client = client
    }

    /// The command shown (and runnable) when the module has no managed
    /// binary yet.
    var setupCommand: String { "symbrain setup --from-source --modules operate" }

    /// The command shown when the module is disabled in config.
    var enableCommand: String { "symbrain config set modules.operate true" }

    /// Loads everything the Operate screen shows on its own, without
    /// triggering a screenshot, an accessibility query, or a network call.
    func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        moduleEnabled = (try? await ManagedModuleSupport.isEnabled(module: "operate", symbrain: symbrain)) ?? false

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
        profileExposure = await ManagedModuleSupport.profileExposure(server: "operate", symbrain: symbrain)
    }

    /// Reads the symbrain broker audit log and keeps the operate server's calls.
    func loadActivity() async {
        brokerActivity = await auditReader.read(profile: nil, server: "operate", limit: 500)
    }

    /// Runs `symoperate version` — contacts GitHub to check for an update.
    func checkVersion() async {
        isCheckingVersion = true
        defer { isCheckingVersion = false }
        do {
            versionReport = try await client.version()
        } catch {
            report(error)
        }
    }

    /// Runs `symoperate doctor` — takes a real screenshot and queries the
    /// frontmost app's UI tree.
    func runDoctor() async {
        isRunningDoctor = true
        defer { isRunningDoctor = false }
        do {
            doctorReport = try await client.doctor()
        } catch {
            report(error)
        }
    }

    /// Builds symoperate from the in-repo receiving copy and installs it,
    /// then reloads status. Requires the module to already be enabled in
    /// config — bypassing that here would defeat the "no exposure from
    /// installation alone" contract (PB-2026-09-09 §7) even though the CLI's
    /// own `--modules` flag does not itself enforce it.
    func buildAndInstall() async {
        guard moduleEnabled else { return }
        isBuilding = true
        clearError()
        defer { isBuilding = false }
        do {
            let outcome = try await ManagedModuleSupport.setupFromSource(module: "operate", symbrain: symbrain)
            buildResult = outcome.firstResult
            if let failure = outcome.firstResult, failure.status == "error" {
                errorMessage = failure.error ?? "Build failed."
            }
            await refresh()
        } catch {
            report(error)
        }
    }
}
#endif
