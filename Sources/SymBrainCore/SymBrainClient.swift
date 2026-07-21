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
        runner: CLIRunner = CLIRunner()
    ) {
        self.runner = runner
        self.locator = BinaryLocator(
            bundle: nil,
            userOverride: userOverride,
            extraDirectories: [
                "/opt/homebrew/bin",
                "/usr/local/bin",
            ]
        )
    }

    // MARK: - Binary resolution

    /// Resolve the symbrain binary URL. Checks user override, PATH,
    /// Homebrew prefixes, and a repo-local dev fallback.
    public func resolveBinary() -> URL? {
        if let located = locator.locate("symbrain") {
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
            throw CLIRunnerError.executionFailed(code: result.exitCode, stderr: result.stderrText)
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
            throw CLIRunnerError.executionFailed(code: result.exitCode, stderr: result.stderrText)
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
            throw CLIRunnerError.executionFailed(code: result.exitCode, stderr: result.stderrText)
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
            throw CLIRunnerError.executionFailed(code: result.exitCode, stderr: result.stderrText)
        }
        return result.stdoutText
    }
}
#endif
