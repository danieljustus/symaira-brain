// VaultClient — typed client over the `symvault` CLI.
//
// The vault is passphrase-locked: every read command fails until a session is
// unlocked, so the client exposes an explicit lock state that the UI drives.
// The passphrase is only ever written to the child process' stdin — never to
// the argument vector, where `ps` would expose it.

#if os(macOS)
import Foundation
import SymairaCLIRunner
import SymairaToolKit

/// The runtime state of the local vault as far as the CLI reports it.
public enum VaultAvailability: Sendable, Equatable {
    /// Not determined yet.
    case checking
    /// The `symvault` binary is not installed.
    case missing
    /// Installed, but no unlocked session.
    case locked
    /// Installed and unlocked — entries can be read.
    case ready
    /// Installed, but the check itself failed.
    case failed(String)
}

/// Locates and executes the `symvault` CLI binary.
public struct VaultClient: Sendable {
    public static let binaryName = "symvault"
    /// Homebrew formula shown when the binary is missing.
    public static let homebrewFormula = "danieljustus/tap/symvault"

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

    /// Base arguments shared by every invocation: colour codes would end up in
    /// the parsed output, and an optional profile selects a named vault.
    private func arguments(profile: String?, command: [String]) -> [String] {
        var result = ["--color", "never"]
        if let profile = profile?.trimmingCharacters(in: .whitespacesAndNewlines),
           !profile.isEmpty {
            result += ["--profile", profile]
        }
        return result + command
    }

    // MARK: - version

    /// Run `symvault version`. The command has no JSON mode, so the raw first
    /// line is returned.
    public func version() async throws -> String {
        let result = try await runner.runAllowingFailure(
            try executable(),
            arguments: arguments(profile: nil, command: ["version"]),
            timeout: 10
        )
        return result.stdoutText
            .components(separatedBy: .newlines)
            .first?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    // MARK: - session

    /// Run `symvault unlock --check`: exit code 0 means an active session.
    public func isUnlocked(profile: String? = nil) async throws -> Bool {
        let result = try await runner.run(
            try executable(),
            arguments: arguments(profile: profile, command: ["unlock", "--check"]),
            timeout: 10
        )
        return result.exitCode == 0
    }

    /// Resolve the current availability without throwing.
    public func availability(profile: String? = nil) async -> VaultAvailability {
        guard isInstalled else { return .missing }
        do {
            return try await isUnlocked(profile: profile) ? .ready : .locked
        } catch {
            return .failed(formatError(error).message)
        }
    }

    /// Run `symvault unlock --ttl <ttl>`, passing the passphrase on stdin.
    public func unlock(
        passphrase: String,
        ttl: String = "15m",
        profile: String? = nil
    ) async throws {
        _ = try await runner.runChecked(
            try executable(),
            arguments: arguments(
                profile: profile,
                command: ["unlock", "--ttl", ttl, "--no-pipe-warning"]
            ),
            stdin: Data((passphrase + "\n").utf8),
            timeout: 60
        )
    }

    /// Run `symvault lock`, ending the session.
    public func lock(profile: String? = nil) async throws {
        _ = try await runner.runChecked(
            try executable(),
            arguments: arguments(profile: profile, command: ["lock"]),
            timeout: 10
        )
    }

    // MARK: - entries

    /// Run `symvault list --output json`.
    public func list(profile: String? = nil) async throws -> [VaultEntrySummary] {
        try await decodeList(
            VaultEntrySummary.self,
            arguments: arguments(profile: profile, command: ["list", "--output", "json"]),
            timeout: 30
        )
    }

    /// Run `symvault find <query> --output json`.
    public func find(query: String, profile: String? = nil) async throws -> [VaultEntrySummary] {
        try await decodeList(
            VaultEntrySummary.self,
            arguments: arguments(profile: profile, command: ["find", query, "--output", "json"]),
            timeout: 30
        )
    }

    /// Run `symvault get <path> --output json`.
    ///
    /// The result contains the entry's secrets — keep it out of logs and off
    /// disk, and mask it in the UI until the user asks to reveal it.
    public func entry(path: String, profile: String? = nil) async throws -> VaultEntryDetail {
        let stdout = try await runner.runChecked(
            try executable(),
            arguments: arguments(profile: profile, command: ["get", path, "--output", "json"]),
            timeout: 30
        )
        do {
            return try JSONDecoder().decode(VaultEntryDetail.self, from: stdout)
        } catch {
            throw CLIRunnerError.invalidJSON(description: String(describing: error))
        }
    }

    // MARK: - doctor

    /// Run `symvault doctor` and return its report as text (no JSON mode).
    public func doctor(profile: String? = nil) async throws -> String {
        let result = try await runner.runAllowingFailure(
            try executable(),
            arguments: arguments(profile: profile, command: ["doctor"]),
            timeout: 60
        )
        return reportText(stdout: result.stdoutText, stderr: result.stderrText)
    }

    // MARK: - Private

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
