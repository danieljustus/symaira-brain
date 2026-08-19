// MemoryClient — typed client over the `symmemory` CLI.
//
// Mirrors SymBrainClient: BinaryLocator for discovery, SymairaCLIRunner for
// execution.  JSON is decoded with a plain decoder and explicit CodingKeys so
// free-form metadata keys survive verbatim (see ModuleModels).

#if os(macOS)
import Foundation
import SymairaCLIRunner
import SymairaToolKit

/// Locates and executes the `symmemory` CLI binary.
public struct MemoryClient: Sendable {
    public static let binaryName = "symmemory"

    public let locator: BinaryLocator
    public let runner: CLIRunner

    public init(
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

    // MARK: - Binary resolution

    /// Resolve the symmemory binary URL. Finder-launched apps have no shell
    /// PATH, so the Homebrew prefixes carry the search; an unverified match is
    /// accepted for the same reason as in SymBrainClient.
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

    /// Run `symmemory version --json`.
    public func version() async throws -> VersionInfo {
        try await runner.runDecoding(
            VersionInfo.self,
            executable: try executable(),
            arguments: ["version", "--json"],
            timeout: 10
        )
    }

    // MARK: - doctor

    /// Run `symmemory doctor` and return its report as text.
    ///
    /// `doctor` has no JSON mode and exits non-zero when a check fails while
    /// still printing the complete report on stdout, so the report is
    /// surfaced either way. stderr is only used when stdout is empty: on a
    /// failing check the CLI also dumps its usage text there.
    public func doctor() async throws -> String {
        let result = try await runner.runAllowingFailure(
            try executable(),
            arguments: ["doctor", "--no-color"],
            timeout: 60
        )
        return reportText(stdout: result.stdoutText, stderr: result.stderrText)
    }

    // MARK: - memories

    /// Run `symmemory list -o json [-s <scope>]`.
    public func list(scope: String? = nil) async throws -> [MemoryRecord] {
        var arguments = ["list", "-o", "json"]
        if let scope, !scope.isEmpty {
            arguments += ["-s", scope]
        }
        return try await decodeList(MemoryRecord.self, arguments: arguments, timeout: 60)
    }

    /// Run `symmemory search <query> -o json [-s <scope>] -l <limit>`.
    public func search(
        query: String,
        scope: String? = nil,
        limit: Int = 25
    ) async throws -> [MemorySearchHit] {
        var arguments = ["search", query, "-o", "json", "-l", String(limit)]
        if let scope, !scope.isEmpty {
            arguments += ["-s", scope]
        }
        return try await decodeList(MemorySearchHit.self, arguments: arguments, timeout: 60)
    }

    /// Run `symmemory set <content> -s <scope> [--kind <kind>] [--staged]`.
    @discardableResult
    public func set(
        content: String,
        scope: String,
        kind: String? = nil,
        staged: Bool = false
    ) async throws -> String {
        var arguments = ["set", content, "-s", scope, "--author", "gui:symbrain"]
        if let kind, !kind.isEmpty {
            arguments += ["--kind", kind]
        }
        if staged {
            arguments.append("--staged")
        }
        let stdout = try await runner.runChecked(
            try executable(),
            arguments: arguments,
            timeout: 120
        )
        return String(data: stdout, encoding: .utf8) ?? ""
    }

    /// Run `symmemory delete <id>`.
    @discardableResult
    public func delete(id: String) async throws -> String {
        let stdout = try await runner.runChecked(
            try executable(),
            arguments: ["delete", id],
            timeout: 30
        )
        return String(data: stdout, encoding: .utf8) ?? ""
    }

    // MARK: - rules

    /// Run `symmemory rule list -o json`.
    public func rules() async throws -> [MemoryRule] {
        try await decodeList(MemoryRule.self, arguments: ["rule", "list", "-o", "json"], timeout: 30)
    }

    // MARK: - query log

    /// Run `symmemory query-log --json --limit <limit>`.
    ///
    /// `query-log` predates the global `-o` flag and takes its own `--json`.
    public func queryLog(limit: Int = 50) async throws -> MemoryQueryLog {
        try await decode(
            MemoryQueryLog.self,
            arguments: ["query-log", "--json", "--limit", String(limit)],
            timeout: 30
        )
    }

    // MARK: - Private

    /// Run a command that returns a JSON array and decode it.  An empty store
    /// makes the CLI print `null` or nothing at all; both mean "no rows".
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

    /// Run a command and decode its stdout with a plain JSON decoder.
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
}
#endif
