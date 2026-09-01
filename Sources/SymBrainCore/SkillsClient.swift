// SkillsClient — typed client over `symbrain skills`.
//
// Like memory, skills is a core embedded in the symbrain binary rather than
// a tool of its own: this client only builds arguments for the same
// `symbrain` executable SymBrainClient runs. No second binary, no second
// version, no separate install.

#if os(macOS)
import Foundation
import SymairaCLIRunner

/// Runs the `symbrain skills` subcommands.
public struct SkillsClient: Sendable {
    /// The shared symbrain client — skills has no binary of its own.
    public let brain: SymBrainClient

    public init(brain: SymBrainClient = SymBrainClient()) {
        self.brain = brain
    }

    // MARK: - Binary resolution

    public var isInstalled: Bool { brain.resolveBinary() != nil }

    private func executable() throws -> URL {
        guard let binary = brain.resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return binary
    }

    // MARK: - doctor

    /// Run `symbrain skills doctor --output json`.
    public func doctor() async throws -> SkillsDoctorReport {
        try await decode(
            SkillsDoctorReport.self,
            arguments: ["skills", "doctor", "--output", "json"],
            timeout: 60
        )
    }

    // MARK: - library

    /// Run `symbrain skills list --output json`.
    public func library() async throws -> SkillLibrary {
        try await decode(
            SkillLibrary.self,
            arguments: ["skills", "list", "--output", "json"],
            timeout: 30
        )
    }

    // MARK: - install status

    /// Run `symbrain skills status --output json [--target <target>]`.
    public func status(target: String? = nil) async throws -> SkillStatusReport {
        var arguments = ["skills", "status", "--output", "json"]
        if let target, !target.isEmpty, target != "all" {
            arguments += ["--target", target]
        }
        return try await decode(SkillStatusReport.self, arguments: arguments, timeout: 60)
    }

    // MARK: - harness targets

    /// Run `symbrain skills targets --output json`.
    public func targets() async throws -> SkillTargetsReport {
        try await decode(
            SkillTargetsReport.self,
            arguments: ["skills", "targets", "--output", "json"],
            timeout: 30
        )
    }

    // MARK: - operation log

    /// Run `symbrain skills log --output json`.
    public func log() async throws -> [SkillLogEntry] {
        try await decodeList(
            SkillLogEntry.self,
            arguments: ["skills", "log", "--output", "json"],
            timeout: 30
        )
    }

    // MARK: - sync

    /// Run `symbrain skills sync --output json [--dry-run] [--target <target>]`.
    ///
    /// Without `dryRun` this writes into the harness skill roots, so the UI
    /// must ask before calling it.
    public func sync(dryRun: Bool, target: String? = nil) async throws -> SkillSyncReport {
        var arguments = ["skills", "sync", "--output", "json"]
        if dryRun { arguments.append("--dry-run") }
        if let target, !target.isEmpty, target != "all" {
            arguments += ["--target", target]
        }
        return try await decode(SkillSyncReport.self, arguments: arguments, timeout: 300)
    }

    // MARK: - Private

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
}
#endif
