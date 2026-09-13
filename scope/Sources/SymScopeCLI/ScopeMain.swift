import Foundation
import SymScopeCore
import SymScopeMCP

/// Scope CLI logic as a library so both the legacy `symscope` binary and the
/// unified `symcockpit scope` dispatcher can invoke it.
///
/// Argument routing by hand (no third-party dependency), mirroring the Go
/// cobra command surface.
public enum ScopeMain {
    /// Runs one command. Returns the process exit code (0 ok, 1 error,
    /// 2 usage). Never calls exit() itself — except inside `watch`, which
    /// streams until interrupted, and `serve`, which blocks on stdio.
    @discardableResult
    public static func run(_ args: [String]) async -> Int32 {
        do {
            return try await runThrowing(args)
        } catch {
            fputs("symscope: \(error.localizedDescription)\n", stderr)
            return 1
        }
    }

    static func runThrowing(_ args: [String]) async throws -> Int32 {
        guard let first = args.first else {
            printUsage()
            return 2
        }

        switch first {
        case "version", "--version", "-V":
            if args.contains("--json") {
                try printJSON(Version.info(), pretty: false)
            } else {
                print("symscope \(Version.version) (schema_version \(Version.info().schemaVersion))")
            }
            return 0

        case "scan":
            let snapshot = await SnapshotService.build()
            try printJSON(snapshot)
            return 0

        case "ports":
            guard args.count >= 2 else { printUsage(); return 2 }
            switch args[1] {
            case "list":
                let ports = try await PortService.listListening()
                try printJSON(ports)
                return 0
            case "suggest":
                let count = args.count >= 3 ? (Int(args[2]) ?? 3) : 3
                let ports = try await PortService.suggestFree(count: count)
                try printJSON(ports)
                return 0
            default:
                printUsage()
                return 2
            }

        case "mcp":
            guard args.count >= 2 else { printUsage(); return 2 }
            switch args[1] {
            case "list":
                let (servers, notes) = MCPDiscovery.discover()
                if let note = notes.first(where: { $0.contains("requires symbrain") }) {
                    fputs("symscope: \(note)\n", stderr)
                    return 1
                }
                try printJSON(servers)
                return 0
            case "health":
                let service = SymBrainHarnessService()
                guard service.isAvailable else {
                    fputs("symscope: \(MCPDiscovery.requiresSymbrainNote)\n", stderr)
                    return 1
                }
                guard let results = service.health() else {
                    fputs("symscope: mcp: symbrain harness health failed\n", stderr)
                    return 1
                }
                try printJSON(results)
                return 0
            default:
                printUsage()
                return 2
            }

        case "daemons":
            guard args.count >= 2 else { printUsage(); return 2 }
            let includeApple = args.dropFirst(2).contains("--all")
            switch args[1] {
            case "list":
                let (daemons, _) = await DaemonService.list(all: includeApple)
                try printJSON(daemons)
                return 0
            case "health":
                let (daemons, _) = await DaemonService.list(all: includeApple)
                try printJSON(DaemonService.health(daemons))
                return 0
            default:
                printUsage()
                return 2
            }

        case "containers":
            let (containers, _) = await ContainerService.list()
            try printJSON(containers)
            return 0

        case "watch":
            guard args.count >= 3, args[1] == "--interval", let interval = Double(args[2]), interval > 0 else {
                fputs("symscope: watch requires --interval <seconds>\n", stderr)
                return 2
            }
            var old = await SnapshotService.build()
            try printJSON(old, pretty: false)
            while true {
                try await Task.sleep(nanoseconds: UInt64(interval * 1_000_000_000))
                let new = await SnapshotService.build()
                let events = WatchService.diff(old: old, new: new)
                for event in events {
                    try printJSON(event, pretty: false)
                }
                old = new
            }

        case "cache":
            guard args.count >= 2 else { printUsage(); return 2 }
            switch args[1] {
            case "show":
                try printJSON(CacheService.stats())
                return 0
            case "clear":
                if let error = CacheService.clear() {
                    fputs("symscope: \(error)\n", stderr)
                    return 1
                }
                print("Cache cleared.")
                return 0
            default:
                printUsage()
                return 2
            }

        case "explain":
            guard args.count >= 3 else { printUsage(); return 2 }
            switch args[1] {
            case "port":
                guard let port = Int(args[2]) else {
                    fputs("symscope: explain port requires a numeric port\n", stderr)
                    return 2
                }
                let ports = try await PortService.listListening()
                let (servers, _) = MCPDiscovery.discover()
                try printJSON(ExplainService.explainPort(port, ports: ports, servers: servers))
                return 0
            case "server":
                let (servers, _) = MCPDiscovery.discover()
                guard let explanation = ExplainService.explainServer(args[2], servers: servers) else {
                    fputs("symscope: unknown MCP server '\(args[2])'\n", stderr)
                    return 1
                }
                try printJSON(explanation)
                return 0
            default:
                printUsage()
                return 2
            }

        case "conflicts":
            let ports = try await PortService.listListening()
            let (daemons, _) = await DaemonService.list()
            let conflicts = ConflictDetector.detect(ports, daemons: daemons)
            try printJSON(conflicts)
            return 0

        case "serve":
            // MCP server transport; stdout is JSON-RPC frames only.
            let server = SymScopeMCPServer()
            try server.runServe()
            return 0

        case "help", "--help", "-h":
            printUsage(to: .standardOutput)
            return 0

        default:
            printUsage()
            return 2
        }
    }

    // MARK: - Output helpers

    // Encoder for output. Models declare explicit snake_case CodingKeys, so
    // no key strategy conversion is applied (that would double-convert and
    // break keys like generated_at / config_path).
    static func makeEncoder(pretty: Bool = false) -> JSONEncoder {
        let encoder = JSONEncoder()
        if pretty {
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        }
        return encoder
    }

    static func printJSON<T: Encodable>(_ value: T, pretty: Bool = true) throws {
        let encoder = makeEncoder(pretty: pretty)
        let data = try encoder.encode(value)
        print(String(data: data, encoding: .utf8) ?? "{}")
    }

    static func printUsage(to output: FileHandle) {
        let text = """
        symscope — inventory local ports, containers, background services, and MCP servers

        Usage:
          symcockpit scope version               print version info (--json)
          symcockpit scope scan                  aggregate snapshot (ports, MCP, containers)
          symcockpit scope ports list            listening ports (JSON)
          symcockpit scope ports suggest [n]     suggest free TCP ports (default 3)
          symcockpit scope mcp list              MCP servers across AI clients (JSON)
          symcockpit scope mcp health            health-probe configured MCP servers
          symcockpit scope daemons list [--all]  launchd agents and Homebrew services
          symcockpit scope daemons health [--all] daemon health summary
          symcockpit scope containers            running Docker containers (JSON)
          symcockpit scope conflicts             ports held by multiple processes
          symcockpit scope watch --interval <s>  watch for changes (NDJSON events)
          symcockpit scope cache show|clear      inspect/clear the snapshot cache
          symcockpit scope explain port|server   explain what uses a port or server
          symcockpit scope serve                 run the stdio MCP server

        (Legacy alias: symscope <command> works identically.)
        """
        output.write(Data(text.utf8))
    }

    public static func printUsage() {
        printUsage(to: .standardError)
    }
}
