// SkillsClient — typed client over the `symskills` CLI.
//
// Same shape as MemoryClient: BinaryLocator for discovery, SymairaCLIRunner
// for execution, plain JSON decoding with explicit CodingKeys.

#if os(macOS)
import Foundation
import SymairaCLIRunner
import SymairaToolKit

/// Locates and executes the `symskills` CLI binary.
public struct SkillsClient: Sendable {
    public static let binaryName = "symskills"

    public let locator: BinaryLocator
    public let runner: CLIRunner

    public init(
        userOverride: URL? = nil,
        searchPATH: String? = nil,
        extraDirectories: [String] = [
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

    // MARK: - Binary resolution

    public func resolveBinary() -> URL? {
        locator.locate(Self.binaryName, allowUnverified: true)?.url
    }

    public var isInstalled: Bool { resolveBinary() != nil }

    private func executable() throws -> URL {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: Self.binaryName)
        }
        return binary
    }

    // MARK: - version

    /// Run `symskills version`. The command has no JSON mode, so the first
    /// output line is returned as-is (e.g. "symskills 0.3.1").
    public func version() async throws -> String {
        let result = try await runner.runAllowingFailure(
            try executable(),
            arguments: ["version"],
            timeout: 10
        )
        return result.stdoutText
            .components(separatedBy: .newlines)
            .first?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    // MARK: - library

    /// Run `symskills list --json`.
    public func library() async throws -> SkillLibrary {
        try await decode(SkillLibrary.self, arguments: ["list", "--json"], timeout: 30)
    }

    // MARK: - install status

    /// Run `symskills status --json [--target <target>]`.
    ///
    /// `status` exits non-zero only with `--strict`, which is not passed here.
    public func status(target: String? = nil) async throws -> SkillStatusReport {
        var arguments = ["status", "--json"]
        if let target, !target.isEmpty, target != "all" {
            arguments += ["--target", target]
        }
        return try await decode(SkillStatusReport.self, arguments: arguments, timeout: 60)
    }

    // MARK: - harness targets

    /// Run `symskills targets --json`.
    public func targets() async throws -> SkillTargetsReport {
        try await decode(SkillTargetsReport.self, arguments: ["targets", "--json"], timeout: 30)
    }

    // MARK: - operation log

    /// Run `symskills log --json`.
    public func log() async throws -> [SkillLogEntry] {
        try await decodeList(SkillLogEntry.self, arguments: ["log", "--json"], timeout: 30)
    }

    // MARK: - sync

    /// Run `symskills sync --json [--dry-run] [--target <target>]`.
    ///
    /// Without `dryRun` this writes into the harness skill roots, so the UI
    /// must ask before calling it.
    public func sync(dryRun: Bool, target: String? = nil) async throws -> SkillSyncReport {
        var arguments = ["sync", "--json"]
        if dryRun { arguments.append("--dry-run") }
        if let target, !target.isEmpty, target != "all" {
            arguments += ["--target", target]
        }
        return try await decode(SkillSyncReport.self, arguments: arguments, timeout: 300)
    }

    // MARK: - doctor

    /// Run `symskills doctor --json`.
    public func doctor() async throws -> SkillsDoctorReport {
        try await decode(SkillsDoctorReport.self, arguments: ["doctor", "--json"], timeout: 60)
    }

    // MARK: - Private

    private func decode<T: Decodable>(
        _ type: T.Type,
        arguments: [String],
        timeout: Double
    ) async throws -> T {
        let stdout = try await runner.runChecked(
            try executable(),
            arguments: arguments,
            timeout: timeout
        )
        do {
            return try JSONDecoder().decode(T.self, from: stdout)
        } catch {
            throw CLIRunnerError.invalidJSON(description: String(describing: error))
        }
    }

    private func decodeList<E: Decodable>(
        _ element: E.Type,
        arguments: [String],
        timeout: Double
    ) async throws -> [E] {
        let stdout = try await runner.runChecked(
            try executable(),
            arguments: arguments,
            timeout: timeout
        )
        let text = String(data: stdout, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if text.isEmpty || text == "null" { return [] }
        do {
            return try JSONDecoder().decode([E].self, from: stdout)
        } catch {
            throw CLIRunnerError.invalidJSON(description: String(describing: error))
        }
    }
}
#endif
