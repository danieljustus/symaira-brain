// SymBrainCore — typed client over the symbrain CLI.
//
// Uses SymairaCLIRunner for subprocess execution with timeouts and
// snake_case JSON decoding, and BinaryLocator for binary discovery.

#if os(macOS)
import Foundation
import SymairaCLIRunner
import SymairaToolKit

/// Locates and executes the `symbrain` CLI binary.
public struct SymBrainClient: Sendable {
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

    /// Resolve the symbrain binary URL. Checks user override, PATH,
    /// Homebrew prefixes, and a repo-local dev fallback.
    public func resolveBinary() -> URL? {
        // BinaryLocator must be allowed to finish its complete search before
        // trying the development fallback. In particular, Finder-launched
        // apps have an empty PATH, so the Homebrew directories are essential.
        // Homebrew's user-managed prefix can be group-writable on macOS,
        // which is expected for a Homebrew installation. The locator still
        // verifies the executable when possible; allow it to return the
        // candidate so Finder-launched apps can use the documented install.
        if let located = locator.locate("symbrain", allowUnverified: true) {
            return located.url
        }
        // Dev fallback: binary sitting next to the app source
        let devPath = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("symbrain")
        if FileManager.default.isExecutableFile(atPath: devPath.path) {
            return devPath
        }
        return nil
    }

    /// A user-facing explanation of the complete automatic search.
    ///
    /// This deliberately does not suggest PATH as the primary remedy: a
    /// Finder-launched app commonly has no shell PATH at all.
    public var binarySearchDiagnostic: String {
        var lines = ["The “symbrain” CLI binary was not found. Searched directories:"]
        if let userOverride = locator.userOverride {
            let status = FileManager.default.isExecutableFile(
                atPath: userOverride.path
            ) ? "found" : "not found"
            lines.append("- Binary Path Override (\(userOverride.path)): \(status)")
        }
        let pathDirectories = locator.searchPATH
            .split(separator: ":")
            .map(String.init)
        if pathDirectories.isEmpty {
            lines.append("- PATH (empty)")
        } else {
            lines += pathDirectories.map { directory in
                "- \(directory): \(binaryStatus(in: directory))"
            }
        }
        lines += locator.extraDirectories.map { directory in
            "- \(directory): \(binaryStatus(in: directory))"
        }
        let devDirectory = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .path
        lines.append("- \(devDirectory): \(binaryStatus(in: devDirectory)) (development fallback)")
        lines.append("Install SymBrain with Homebrew or set a Binary Path Override in Settings.")
        return lines.joined(separator: "\\n")
    }

    private func binaryStatus(in directory: String) -> String {
        let path = URL(fileURLWithPath: directory).appendingPathComponent("symbrain").path
        return FileManager.default.isExecutableFile(atPath: path) ? "found" : "not found"
    }

    // MARK: - init

    /// Run `symbrain init` to create the initial configuration.
    public func initialize() async throws -> String {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        let result = try await runner.run(binary, arguments: ["init"])
        guard result.exitCode == 0 else {
            throw CLIRunnerError.executionFailed(code: result.exitCode, fullStderr: result.stderrText)
        }
        return result.stdoutText
    }

    // MARK: - version --json

    /// Run `symbrain version --json` and decode the result.
    public func version() async throws -> VersionInfo {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return try await runner.runDecoding(
            VersionInfo.self,
            executable: binary,
            arguments: ["version", "--json"]
        )
    }

    // MARK: - doctor --json

    /// Run `symbrain doctor --json` and decode the result.
    public func doctor() async throws -> DoctorReport {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return try await runner.runDecoding(
            DoctorReport.self,
            executable: binary,
            arguments: ["doctor", "--json"]
        )
    }

    // MARK: - profile list --json

    /// Run `symbrain profile list --json` and decode the result.
    public func profileList() async throws -> [ProfileSummary] {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return try await runner.runDecoding(
            [ProfileSummary].self,
            executable: binary,
            arguments: ["profile", "list", "--json"]
        )
    }

    // MARK: - profile show <name> --json

    /// Run `symbrain profile show <name> --json` and decode the result.
    public func profileShow(name: String) async throws -> ProfileDetail {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return try await runner.runDecoding(
            ProfileDetail.self,
            executable: binary,
            arguments: ["profile", "show", name, "--json"]
        )
    }

    // MARK: - profile add / remove (text output)

    /// Run `symbrain profile add <name> [--from <template>]`.
    public func profileAdd(name: String, from template: String? = nil) async throws -> String {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        var args = ["profile", "add", name]
        if let template {
            args += ["--from", template]
        }
        let result = try await runner.run(binary, arguments: args)
        guard result.exitCode == 0 else {
            throw CLIRunnerError.executionFailed(code: result.exitCode, fullStderr: result.stderrText)
        }
        return result.stdoutText
    }

    /// Run `symbrain profile remove <name> --force`.
    public func profileRemove(name: String) async throws -> String {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        let result = try await runner.run(binary, arguments: ["profile", "remove", name, "--force"])
        guard result.exitCode == 0 else {
            throw CLIRunnerError.executionFailed(code: result.exitCode, fullStderr: result.stderrText)
        }
        return result.stdoutText
    }

    // MARK: - install / uninstall

    /// Run `symbrain install --harness <harness> --profile <profile> [--dry-run]`.
    public func install(harness: String, profile: String, dryRun: Bool = false) async throws -> String {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        var args = ["install", "--harness", harness, "--profile", profile]
        if dryRun { args.append("--dry-run") }
        let result = try await runner.run(binary, arguments: args)
        guard result.exitCode == 0 else {
            throw CLIRunnerError.executionFailed(code: result.exitCode, fullStderr: result.stderrText)
        }
        return result.stdoutText
    }

    /// Run `symbrain uninstall --harness <harness> [--dry-run]`.
    public func uninstall(harness: String, dryRun: Bool = false) async throws -> String {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        var args = ["uninstall", "--harness", harness]
        if dryRun { args.append("--dry-run") }
        let result = try await runner.run(binary, arguments: args)
        guard result.exitCode == 0 else {
            throw CLIRunnerError.executionFailed(code: result.exitCode, fullStderr: result.stderrText)
        }
        return result.stdoutText
    }

    // MARK: - sync --json

    /// Run `symbrain sync --json` and decode the result.
    public func sync() async throws -> SyncSummary {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        return try await runner.runDecoding(
            SyncSummary.self,
            executable: binary,
            arguments: ["sync", "--json"]
        )
    }

    /// Run `symbrain sync --json --dry-run` and decode the result.
    public func sync(dryRun: Bool) async throws -> SyncSummary {
        guard let binary = resolveBinary() else {
            throw CLIRunnerError.binaryNotFound(tool: "symbrain")
        }
        var args = ["sync", "--json"]
        if dryRun { args.append("--dry-run") }
        return try await runner.runDecoding(
            SyncSummary.self,
            executable: binary,
            arguments: args
        )
    }
}
#endif
