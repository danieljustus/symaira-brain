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
        try Self.validateValue(value, field: field)
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

    /// Create an entry with the documented non-secret metadata flags.
    public func create(draft: VaultCredentialDraft, profile: String? = nil) async throws -> VaultCreateConfirmation {
        guard Self.validType(draft.type), Self.validPath(draft.path) else {
            throw CLIRunnerError.invalidJSON(description: "invalid credential path or type")
        }
        try Self.validateMetadata(draft)
        try Self.validateSingleLine(draft.secret)
        var command = ["add", draft.path, "--stdin-value", "--type", draft.type]
        Self.append("--username", value: draft.username, to: &command)
        Self.append("--url", value: draft.url, to: &command)
        Self.append("--notes", value: draft.notes, to: &command)
        Self.append("--usage-hint", value: draft.usageHint, to: &command)
        Self.append("--expires-at", value: draft.expiresAt, to: &command)
        if draft.autoRotate { command.append("--auto-rotate") }
        var input = draft.secret + "\n"
        if !draft.totpSecret.isEmpty {
            try Self.validateSingleLine(draft.totpSecret)
            command.append("--stdin-totp-secret")
            Self.append("--totp-issuer", value: draft.totpIssuer, to: &command)
            Self.append("--totp-account", value: draft.totpAccount, to: &command)
            input += draft.totpSecret + "\n"
        }
        _ = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: command), stdin: Data(input.utf8), timeout: 60)
        let confirmed = try await entry(path: draft.path, profile: profile)
        return VaultCreateConfirmation(submittedPath: draft.path, confirmedPath: confirmed.path.isEmpty ? draft.path : confirmed.path, confirmedFieldCount: confirmed.fields.count, confirmedHasValue: confirmed.fields.values.contains { !$0.isEmpty })
    }

    /// Update documented non-secret fields. Type, usage hint, auto-rotate and expiration
    /// are create-time metadata in the current `symvault set` contract and are not guessed here.
    public func update(path: String, original: VaultEntryDetail, draft: VaultCredentialDraft, profile: String? = nil) async throws -> VaultSetConfirmation {
        guard Self.validPath(path) else { throw CLIRunnerError.invalidJSON(description: "invalid vault entry path") }
        let secretField = original.primarySecret?.field
        let updates: [(String, String)] = [
            (secretField ?? "", draft.secret),
            ("username", draft.username),
            ("url", draft.url),
            ("notes", draft.notes)
        ]
        var last: VaultSetConfirmation?
        for (field, value) in updates where !field.isEmpty && ((field == secretField && !value.isEmpty) || field != secretField) && value != original.fields[field]?.displayString {
            last = try await set(path: path, field: field, value: value, profile: profile)
        }
        if let last { return last }
        let confirmed = try await entry(path: path, profile: profile)
        return VaultSetConfirmation(submittedPath: path, submittedField: "", confirmedPath: confirmed.path.isEmpty ? path : confirmed.path, confirmedField: nil, confirmedValueMatches: true, confirmedFieldCount: confirmed.fields.count, confirmedHasValue: confirmed.fields.values.contains { !$0.isEmpty })
    }

    public func generatePassword(length: Int, symbols: Bool, profile: String? = nil) async throws -> String {
        guard (1...1024).contains(length) else {
            throw CLIRunnerError.invalidJSON(description: "password length must be between 1 and 1024")
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

    /// List pending requests through the Vault service.
    public func approvalList(profile: String? = nil) async throws -> [VaultApprovalRequest] {
        let data = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: ["approval", "list", "--output", "json"]), timeout: 15)
        do {
            struct Response: Decodable { let requests: [VaultApprovalRequest] }
            return try JSONDecoder().decode(Response.self, from: data).requests
        } catch { throw CLIRunnerError.invalidJSON(description: "invalid approval list response") }
    }

    /// Decide exactly one request and validate the server outcome.
    public func approvalDecide(id: String, approve: Bool, profile: String? = nil) async throws -> VaultApprovalOutcome {
        guard !id.isEmpty, !id.contains(where: { $0.isWhitespace || $0 == "/" }) else { throw CLIRunnerError.invalidJSON(description: "invalid approval request id") }
        let data = try await runner.runChecked(try executable(), arguments: arguments(profile: profile, command: ["approval", "decide", id, approve ? "--approve" : "--deny"]), timeout: 30)
        do {
            struct Response: Decodable { let outcome: VaultApprovalOutcome }
            let outcome = try JSONDecoder().decode(Response.self, from: data).outcome
            guard outcome.id == id, outcome.status == (approve ? "approved" : "denied") else { throw CLIRunnerError.invalidJSON(description: "approval outcome did not match requested request") }
            return outcome
        } catch let error as CLIRunnerError { throw error }
        catch { throw CLIRunnerError.invalidJSON(description: "invalid approval decision response") }
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

    private static func validateValue(_ value: String, field: String) throws {
        if value.unicodeScalars.contains(where: { $0.value == 0x0A || $0.value == 0x0D }) {
            // `set --stdin-value` is line-framed by symvault; reject before
            // dispatch for every field, including non-sensitive metadata.
            throw CLIRunnerError.invalidJSON(description: VaultFieldSecurity.isSensitive(field) ? "multiline secret values are not supported" : "multiline values are not supported")
        }
        if value.isEmpty {
            guard !VaultFieldSecurity.isSensitive(field) else {
                throw CLIRunnerError.invalidJSON(description: "sensitive field values cannot be empty")
            }
            return
        }
        if VaultFieldSecurity.isSensitive(field) {
            try validateSingleLine(value)
        }
    }

    private static func validateSingleLine(_ value: String) throws {
        guard !value.isEmpty else { throw CLIRunnerError.invalidJSON(description: "secret value is empty") }
        guard !value.unicodeScalars.contains(where: { $0.value == 0x0A || $0.value == 0x0D }) else {
            throw CLIRunnerError.invalidJSON(description: "multiline secret values are not supported")
        }
    }

    private static func validPath(_ path: String) -> Bool {
        guard !path.isEmpty, !path.hasPrefix("/"), !path.hasSuffix("/"),
              !path.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7F }) else { return false }
        return path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy {
            !$0.isEmpty && $0 != "." && $0 != ".."
        }
    }

    private static func validType(_ type: String) -> Bool {
        ["api_key", "bearer_token", "basic_auth", "ssh_key", "password", "certificate", "database_url", "totp_seed", "custom"].contains(type)
    }

    private static func validateMetadata(_ draft: VaultCredentialDraft) throws {
        for value in [draft.username, draft.url, draft.notes, draft.usageHint, draft.expiresAt, draft.totpIssuer, draft.totpAccount] {
            guard !value.unicodeScalars.contains(where: { $0.value < 0x0A || ($0.value > 0x0D && $0.value < 0x20) || $0.value == 0x7F }) else {
                throw CLIRunnerError.invalidJSON(description: "credential metadata contains a control character")
            }
        }
        if !draft.expiresAt.isEmpty && ISO8601DateFormatter().date(from: draft.expiresAt) == nil {
            throw CLIRunnerError.invalidJSON(description: "expiration must be an RFC3339 timestamp")
        }
    }

    private static func append(_ flag: String, value: String, to command: inout [String]) {
        guard !value.isEmpty else { return }
        command += [flag, value]
    }

    private static func validField(_ field: String) -> Bool {
        !field.isEmpty && !field.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7F })
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
        guard ocrTexts.count <= 1 else {
            throw CLIRunnerError.invalidJSON(description: "symvault intake accepts one --ocr-text file per invocation")
        }
        if let ocr = ocrTexts.values.first {
            command += ["--ocr-text", ocr.path]
        }
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
