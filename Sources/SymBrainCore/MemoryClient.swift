// MemoryClient — typed client over `symbrain memory`.
//
// Memory is not a separate tool: it is a core embedded in the symbrain
// binary, and this client is a thin argument builder over the same
// `symbrain` executable SymBrainClient runs. There is no second binary to
// discover, no second version to report, and no separate install step.
//
// JSON is decoded with a plain decoder and explicit CodingKeys so free-form
// metadata keys survive verbatim (see ModuleModels).

#if os(macOS)
import Foundation
import SymairaCLIRunner

/// Runs the `symbrain memory` subcommands.
public struct MemoryClient: Sendable {
    /// The shared symbrain client — memory has no binary of its own.
    public let brain: SymBrainClient

    public init(brain: SymBrainClient = SymBrainClient()) {
        self.brain = brain
    }

    /// How many memories `list` asks the store for. The CLI caps a list at
    /// 1000 rows; the GUI renders a bounded page of that anyway.
    public static let listFetchLimit = 1000

    // MARK: - Binary resolution

    public var isInstalled: Bool { brain.resolveBinary() != nil }

    private func executable() throws -> URL {
        guard let binary = brain.resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return binary
    }

    // MARK: - doctor

    /// Run `symbrain doctor` and return its report as text.
    ///
    /// Memory has no health command of its own — one binary means one health
    /// check, and `symbrain doctor` already reports memory among the cores it
    /// serves. `doctor` has no JSON mode in the table sense used here and
    /// exits non-zero when a check fails while still printing the complete
    /// report on stdout, so the report is surfaced either way.
    public func doctor() async throws -> String {
        let result = try await brain.runner.runAllowingFailure(
            try executable(),
            arguments: ["doctor"],
            timeout: 60
        )
        return reportText(stdout: result.stdoutText, stderr: result.stderrText)
    }

    // MARK: - memories

    /// Run `symbrain memory list --output json [--scope <scope>]`.
    public func list(scope: String? = nil) async throws -> [MemoryRecord] {
        var arguments = ["memory", "list", "--output", "json", "--limit", String(Self.listFetchLimit)]
        if let scope, !scope.isEmpty {
            arguments += ["--scope", scope]
        }
        return try await decodeList(MemoryRecord.self, arguments: arguments, timeout: 60)
    }

    /// Run `symbrain memory search <query> --output json [--scope <scope>]`.
    public func search(
        query: String,
        scope: String? = nil,
        limit: Int = 25
    ) async throws -> [MemorySearchHit] {
        var arguments = ["memory", "search", query, "--output", "json", "--limit", String(limit)]
        if let scope, !scope.isEmpty {
            arguments += ["--scope", scope]
        }
        return try await decodeList(MemorySearchHit.self, arguments: arguments, timeout: 60)
    }

    /// Run `symbrain memory set <content> --scope <scope> [--kind <kind>] [--staged]`.
    @discardableResult
    public func set(
        content: String,
        scope: String,
        kind: String? = nil,
        staged: Bool = false
    ) async throws -> String {
        var arguments = ["memory", "set", content, "--scope", scope, "--author", "gui:symbrain"]
        if let kind, !kind.isEmpty {
            arguments += ["--kind", kind]
        }
        if staged {
            arguments.append("--staged")
        }
        let stdout = try await brain.runner.runChecked(
            try executable(),
            arguments: arguments,
            timeout: 120
        )
        return String(data: stdout, encoding: .utf8) ?? ""
    }

    /// Run `symbrain memory delete <id>`.
    @discardableResult
    public func delete(id: String) async throws -> String {
        let stdout = try await brain.runner.runChecked(
            try executable(),
            arguments: ["memory", "delete", id],
            timeout: 30
        )
        return String(data: stdout, encoding: .utf8) ?? ""
    }

    // MARK: - rules

    /// Run `symbrain memory rules --output json`.
    public func rules() async throws -> [MemoryRule] {
        try await decodeList(
            MemoryRule.self,
            arguments: ["memory", "rules", "--output", "json"],
            timeout: 30
        )
    }

    // MARK: - query log

    /// Run `symbrain memory query-log --output json --limit <limit>`.
    public func queryLog(limit: Int = 50) async throws -> MemoryQueryLog {
        try await decode(
            MemoryQueryLog.self,
            arguments: ["memory", "query-log", "--output", "json", "--limit", String(limit)],
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
        let stdout = try await brain.runner.runChecked(
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
        let stdout = try await brain.runner.runChecked(
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
