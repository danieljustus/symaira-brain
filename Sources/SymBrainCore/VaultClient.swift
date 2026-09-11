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
    public func version(profile: String? = nil) async throws -> String {
        let result = try await runner.runAllowingFailure(
            try executable(),
            arguments: arguments(profile: profile, command: ["version"]),
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

    /// Create an entry using symvault's stdin-only value flag, then re-read
    /// it from the service and return metadata only.
    public func create(path: String, value: String, profile: String? = nil) async throws -> VaultCreateConfirmation {
        guard Self.validPath(path) else { throw CLIRunnerError.invalidJSON(description: "invalid vault entry path") }
        try Self.validateSingleLine(value)
        _ = try await runner.runChecked(
            try executable(),
            arguments: arguments(profile: profile, command: ["add", path, "--stdin-value"]),
            stdin: Data((value + "\n").utf8),
            timeout: 60
        )
        let confirmed = try await entry(path: path, profile: profile)
        let hasValue = confirmed.fields.values.contains { !$0.isEmpty }
        return VaultCreateConfirmation(
            submittedPath: path, confirmedPath: confirmed.path.isEmpty ? path : confirmed.path,
            confirmedFieldCount: confirmed.fields.count, confirmedHasValue: hasValue
        )
    }

    /// Update an entry using symvault's stdin-only value flag, then re-read metadata.
    public func set(path: String, field: String, value: String, profile: String? = nil) async throws -> VaultSetConfirmation {
        guard Self.validPath(path), Self.validField(field) else {
            throw CLIRunnerError.invalidJSON(description: "invalid vault entry path or field")
        }
        try Self.validateSingleLine(value)
        let target = path + "." + field
        _ = try await runner.runChecked(
            try executable(), arguments: arguments(profile: profile, command: ["set", target, "--stdin-value"]),
            stdin: Data((value + "\n").utf8), timeout: 60
        )
        let confirmed = try await entry(path: path, profile: profile)
        let confirmedValue = confirmed.fields[field]?.displayString
        guard confirmedValue == value else {
            throw CLIRunnerError.invalidJSON(description: "updated field confirmation did not match requested value")
        }
        return VaultSetConfirmation(submittedPath: path, submittedField: field,
            confirmedPath: confirmed.path.isEmpty ? path : confirmed.path,
            confirmedField: confirmed.fields[field] == nil ? nil : field,
            confirmedValueMatches: true, confirmedFieldCount: confirmed.fields.count,
            confirmedHasValue: confirmed.fields.values.contains { !$0.isEmpty })
    }

    /// Generate a password through the Vault service with explicit options.
    public func generatePassword(length: Int, symbols: Bool, profile: String? = nil) async throws -> String {
        guard (1...4096).contains(length) else {
            throw CLIRunnerError.invalidJSON(description: "password length must be between 1 and 4096")
        }
        var command = ["generate", "--length", String(length)]
        if symbols { command.append("--symbols") }
        let data = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: command), timeout: 10)
        let password = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !password.isEmpty else { throw CLIRunnerError.invalidJSON(description: "symvault returned an empty generated password") }
        return password
    }

    /// Delete an entry with symvault's explicit non-interactive confirmation flag, then verify absence.
    public func delete(path: String, profile: String? = nil) async throws -> VaultDeleteConfirmation {
        _ = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: ["delete", path, "--yes"]), timeout: 60)
        let reread = try await runner.runAllowingFailure(try executable(), arguments: arguments(profile: profile, command: ["get", path, "--output", "json"]), timeout: 30)
        // symvault ExitNotFound = 2 (see symaira-vault internal/errors/errors.go).
        // Only that code proves absence; any other failure leaves the deletion
        // submitted but unverified and must surface as such.
        switch reread.exitCode {
        case 2:
            return VaultDeleteConfirmation(submittedPath: path, confirmedPath: path, confirmedAbsent: true)
        case 0:
            throw CLIRunnerError.invalidJSON(description: "deleted entry is still present")
        default:
            throw CLIRunnerError.invalidJSON(description: "deletion submitted but absence verification failed (symvault get exit \(reread.exitCode))")
        }
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

    private static func validateSingleLine(_ value: String) throws {
        guard !value.isEmpty else { throw CLIRunnerError.invalidJSON(description: "secret value is empty") }
        guard !value.unicodeScalars.contains(where: { $0.value == 0x0A || $0.value == 0x0D }) else {
            throw CLIRunnerError.invalidJSON(description: "multiline secret values are not supported")
        }
    }

    private static func validPath(_ path: String) -> Bool {
        guard !path.isEmpty, !path.hasPrefix("/"), !path.hasSuffix("/") else { return false }
        return path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy {
            !$0.isEmpty && $0.allSatisfy(validTokenCharacter)
        }
    }

    private static func validField(_ field: String) -> Bool {
        !field.isEmpty && field.allSatisfy(validTokenCharacter)
    }

    private static func validTokenCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == "_" || character == "-"
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


    /// Preview file parsing without mutating the Vault.
    public func intakePreview(files: [URL], profile: String? = nil) async throws -> VaultIntakeResponse {
        try await decodeIntake(arguments: arguments(profile: profile, command: ["intake"] + files.map(\.path) + ["--dry-run", "--json"]), timeout: 30)
    }

    /// Stage files into Vault quarantine for human review.
    public func intakeStage(files: [URL], ocrTexts: [URL: URL] = [:], moveToTrash: Bool = false, profile: String? = nil) async throws -> VaultIntakeResponse {
        var command = ["intake"] + files.map(\.path) + ["--json"]
        if moveToTrash { command.append("--move-to-trash") }
        for (source, ocr) in ocrTexts { command += ["--ocr-text", ocr.path, source.path] }
        return try await decodeIntake(arguments: arguments(profile: profile, command: command), timeout: 120)
    }

    public func intakeReviewBatches(profile: String? = nil) async throws -> [String] {
        let data = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: ["import", "review", "list"]), timeout: 15)
        let text = String(data: data, encoding: .utf8) ?? ""
        return text.split(separator: "\n").compactMap { line in
            let id = line.split(separator: " ").first.map(String.init) ?? ""
            return id.isEmpty ? nil : id
        }
    }

    /// Promotion is intentionally separate and requires an explicit import id from review.
    public func intakePromote(importID: String, overwrite: Bool = false, profile: String? = nil) async throws {
        guard !importID.isEmpty, !importID.contains(where: { $0.isWhitespace || $0 == "/" }) else {
            throw CLIRunnerError.invalidJSON(description: "invalid intake import id")
        }
        var command = ["import", "review", "promote", importID]
        if overwrite { command.append("--overwrite") }
        _ = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: command), timeout: 60)
    }

    private func decodeIntake(arguments: [String], timeout: Double) async throws -> VaultIntakeResponse {
        let data = try await runner.runChecked(try executable(), arguments: arguments, timeout: timeout)
        do { return try JSONDecoder().decode(VaultIntakeResponse.self, from: data) }
        catch { throw CLIRunnerError.invalidJSON(description: String(describing: error)) }
    }
}
#endif
