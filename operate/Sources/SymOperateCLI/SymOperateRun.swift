import Foundation
import SymOperateCore
import SymOperateMCP

/// Operate CLI logic as a library so both the legacy `symoperate` binary and
/// the unified `symcockpit operate` dispatcher can invoke it.
public enum OperateMain {
    /// Runs one command. May call exit() directly for streaming commands
    /// (serve, updates check) — same semantics as the legacy binary.
    @discardableResult
    public static func run(_ args: [String]) -> Int32 {
        do {
            guard let first = args.first else {
                printUsage()
                exit(ExitCode.ok.rawValue)
            }

            if first == "--help" || first == "-h" {
                printUsage()
                exit(ExitCode.ok.rawValue)
            }

            let controller: AutomationController
            switch first {
            case Command.serve.rawValue:
                let grantNames = try parseServeGrantArguments(Array(args.dropFirst()))
                controller = AutomationController(
                    actionPolicy: try StartupPolicy.load(grantNames: grantNames)
                )
            case Command.doctor.rawValue:
                controller = AutomationController(
                    actionPolicy: try StartupPolicy.load(grantNames: nil)
                )
            default:
                controller = AutomationController()
            }

            switch first {
    case Command.serve.rawValue:
        // Non-blocking update check on launch — prints to stderr, never blocks.
        Task { @Sendable in
            let checker = UpdateChecker()
            let info = await checker.checkForUpdate()
            if info.updateAvailable, let latest = info.latestVersion, let url = info.releaseURL {
                FileHandle.standardError.write(Data("\n⚠️  Update available: v\(latest) → \(url)\n".utf8))
                FileHandle.standardError.write(Data("   Use `symoperate updates skip \(latest)` to dismiss this version.\n\n".utf8))
            }
        }
        let server = MCPServer(controller: controller)
        Task { @Sendable in
            do {
                try await server.run()
            } catch {
                FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            }
            exit(0)
        }
        dispatchMain()
    case Command.version.rawValue:
        Task { @Sendable in
            let checker = UpdateChecker()
            let update = await checker.checkForUpdate()
            struct VersionReport: Codable {
                let version: String
                let updateAvailable: Bool
                let latestVersion: String?
                let releaseURL: String?
                let error: String?
            }
            try? printJSON(VersionReport(
                version: SymOperateVersion.current,
                updateAvailable: update.updateAvailable,
                latestVersion: update.latestVersion,
                releaseURL: update.releaseURL,
                error: update.error
            ))
            exit(0)
        }
        dispatchMain()
    case Command.doctor.rawValue:
        let permissions = controller.permissionsStatus()
        let apps = controller.listApps()
        let displays = controller.listDisplays()

        let screenshotProbe: ProbeResult
        if permissions.screenRecordingGranted {
            do {
                _ = try controller.snapshot()
                screenshotProbe = ProbeResult(ok: true, message: "Screenshot capture works.")
            } catch let error as AutomationError {
                screenshotProbe = ProbeResult(ok: false, message: DoctorAdvice.screenshotProbeRecommendation(for: error))
            } catch {
                screenshotProbe = ProbeResult(ok: false, message: error.localizedDescription)
            }
        } else {
            screenshotProbe = ProbeResult(ok: false, message: "Screen Recording permission denied.")
        }

        let accessibilityProbe: ProbeResult
        if permissions.accessibilityGranted {
            do {
                let ax = AccessibilityService()
                _ = try ax.queryFrontmostUI(snapshotID: "doctor-probe", maxDepth: 1, maxNodes: 10)
                accessibilityProbe = ProbeResult(ok: true, message: "Accessibility query works.")
            } catch let error as AutomationError {
                accessibilityProbe = ProbeResult(ok: false, message: error.localizedDescription)
            } catch {
                accessibilityProbe = ProbeResult(ok: false, message: error.localizedDescription)
            }
        } else {
            accessibilityProbe = ProbeResult(ok: false, message: "Accessibility permission denied.")
        }

        let platform = ProcessInfo.processInfo.operatingSystemVersion
        let macOSVersion = "\(platform.majorVersion).\(platform.minorVersion).\(platform.patchVersion)"
        let swiftVer = ProcessInfo.processInfo.environment["SWIFT_VERSION"] ?? "unknown"

        let environment = EnvironmentReport(
            platform: "macOS",
            macOSVersion: macOSVersion,
            swiftVersion: swiftVer,
            appsCount: apps.count,
            displaysCount: displays.count
        )

        let capabilities: [String: Bool] = [
            "screenshot": permissions.screenRecordingGranted,
            "accessibility": permissions.accessibilityGranted,
            "multi_display": displays.count > 1,
            "ocr": true,
        ]

        var recommendations: [String] = []
        if !screenshotProbe.ok { recommendations.append(screenshotProbe.message) }
        if !accessibilityProbe.ok { recommendations.append(accessibilityProbe.message) }

        // Detect if launched from a terminal or IDE rather than an MCP host.
        // When that happens, the TCC grants belong to the terminal/IDE, not to
        // the MCP host that will run symoperate in production.
        if let parentName = permissions.source.launchingProcessName?.lowercased() {
            let terminalPatterns = ["terminal", "iterm", "zsh", "bash", "fish", "tmux", "apple_terminal"]
            let idePatterns = ["code", "cursor", "xcode", "clion", "intellij", "rider", "android studio"]
            let isTerminal = terminalPatterns.contains { parentName.contains($0) }
            let isIDE = idePatterns.contains { parentName.contains($0) }
            if isTerminal || isIDE {
                let kind = isTerminal ? "terminal" : "IDE"
                recommendations.append("Launched from a \(kind) (\(permissions.source.launchingProcessName!)). TCC grants reported above belong to the \(kind), not to your MCP host. For production use, grant permissions from the MCP host process (e.g. Claude Desktop, Cursor) and launch symoperate via the host's MCP configuration.")
            }
        }

        if recommendations.isEmpty { recommendations.append("Environment ready.") }

        let ok = screenshotProbe.ok && accessibilityProbe.ok
        try printJSON(DoctorReport(
            ok: ok,
            version: SymOperateVersion.current,
            permissions: permissions,
            capabilities: capabilities,
            environment: environment,
            recommendations: recommendations,
            effectiveGrant: controller.actionPolicy.grantedPermissions.flagNames
        ))
        exit(ok ? ExitCode.ok.rawValue : ExitCode.permissionDenied.rawValue)
    case Command.history.rawValue:
        let rest = Array(args.dropFirst())
        if !rest.contains("--json") {
            FileHandle.standardError.write(Data("error: history currently only supports --json output.\n".utf8))
            exit(ExitCode.invalidArgument.rawValue)
        }
        let historyService = HistoryService()
        let events = try historyService.events()
        try printJSON(events)
        exit(0)
    case Command.permissions.rawValue:
        let rest = Array(args.dropFirst())
        guard let subcommand = rest.first else {
            printUsage()
            exit(ExitCode.invalidArgument.rawValue)
        }

        switch subcommand {
        case "status":
            try printJSON(controller.permissionsStatus())
        case "grant":
            guard let target = rest.dropFirst().first else {
                throw AutomationError.invalidArgument("permissions grant requires 'accessibility' or 'screen'.")
            }
            let success: Bool
            switch target {
            case "accessibility":
                success = controller.requestAccessibilityPermission()
            case "screen":
                success = controller.requestScreenRecordingPermission()
            default:
                throw AutomationError.invalidArgument("Unknown permission target '\(target)'.")
            }
            try printJSON(GrantResult(prompted: success))
        default:
            throw AutomationError.invalidArgument("Unknown permissions subcommand '\(subcommand)'.")
        }
    case Command.updates.rawValue:
        let rest = Array(args.dropFirst())
        guard let subcommand = rest.first else {
            FileHandle.standardError.write(Data("usage: symoperate updates check [--force] | skip [<version>] | clear-skip\n".utf8))
            exit(ExitCode.invalidArgument.rawValue)
        }
        switch subcommand {
        case "check":
            Task { @Sendable in
                let checker = UpdateChecker()
                let force = rest.contains("--force")
                let info = await checker.checkForUpdate(force: force)
                try? printJSON(info)
                exit(0)
            }
            dispatchMain()
        case "skip":
            let version = rest.dropFirst().first
            if let ver = version {
                let checker = UpdateChecker()
                checker.skipVersion(ver)
                let msg = ["skipped": ver]
                try printJSON(msg)
            } else {
                let checker = UpdateChecker()
                if let skipped = checker.skippedVersion {
                    try printJSON(["skipped": skipped])
                } else {
                    try printJSON(["skipped": "none"])
                }
            }
        case "clear-skip":
            let checker = UpdateChecker()
            checker.clearSkippedVersion()
            try printJSON(["skipped": "none"])
        default:
            FileHandle.standardError.write(Data("usage: symoperate updates check [--force] | skip [<version>] | clear-skip\n".utf8))
            exit(ExitCode.invalidArgument.rawValue)
        }
    default:
        printUsage()
        exit(ExitCode.invalidArgument.rawValue)
    }

        return ExitCode.ok.rawValue
        } catch let error as AutomationError {
            FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            return exitCode(for: error).rawValue
        } catch {
            FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            return ExitCode.generalError.rawValue
        }
    }
}

/// Parses only the options owned by `serve`; the options are consumed before
/// the MCP server starts so they never appear on the JSON-RPC stream.
private func parseServeGrantArguments(_ args: [String]) throws -> [String]? {
    var names: [String]?
    var index = 0
    while index < args.count {
        let argument = args[index]
        let value: String
        if argument == "--grant" {
            index += 1
            guard index < args.count else {
                throw AutomationError.invalidArgument("serve --grant requires at least one permission name.")
            }
            value = args[index]
        } else if argument.hasPrefix("--grant=") {
            value = String(argument.dropFirst("--grant=".count))
        } else {
            throw AutomationError.invalidArgument("Unknown serve option '\(argument)'.")
        }

        let parsed = value.split(separator: ",", omittingEmptySubsequences: true).map(String.init)
        guard !parsed.isEmpty else {
            throw AutomationError.invalidArgument("serve --grant requires at least one permission name.")
        }
        if names == nil { names = [] }
        names?.append(contentsOf: parsed)
        index += 1
    }
    return names
}
