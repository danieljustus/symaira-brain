import Foundation

/// MCP server discovery is a thin view over symbrain's schema-2 harness
/// inventory. Cockpit deliberately does not keep a second config parser or
/// fallback list of harness-specific paths.
public enum MCPDiscovery: Sendable {
    public static let requiresSymbrainNote =
        "mcp: requires symbrain; install it with `brew install danieljustus/tap/symbrain`"

    /// Returns discovered servers and non-fatal notes. An unavailable or
    /// incompatible symbrain is represented by an empty result and an
    /// actionable note; there is no local configuration fallback.
    public static func discover(
        harnessService: any HarnessInventoryProviding = SymBrainHarnessService()
    ) -> ([MCPServer], [String]) {
        guard harnessService.isAvailable,
              let inventory = harnessService.list(projectDir: nil) else {
            return ([], [requiresSymbrainNote])
        }
        guard inventory.schemaVersion == 2 else {
            return (
                [],
                ["mcp: requires symbrain schema 2; upgrade symbrain and try again"]
            )
        }
        return discoverViaSymbrain(inventory)
    }

    /// Returns the additive typed view of Brain's inventory. Free-form parser
    /// errors are reduced to stable generic messages; config contents and
    /// resolved environment values are never included.
    public static func diagnose(
        harnessService: any HarnessInventoryProviding = SymBrainHarnessService()
    ) -> (MCPDiscoveryDiagnostics?, [String]) {
        guard harnessService.isAvailable,
              let inventory = harnessService.list(projectDir: nil) else {
            return (nil, [requiresSymbrainNote])
        }
        guard inventory.schemaVersion == 2 else {
            return (
                nil,
                ["mcp: requires symbrain schema 2; upgrade symbrain and try again"]
            )
        }

        let (servers, _) = discoverViaSymbrain(inventory)
        let configurations = inventory.harnesses.flatMap { harness in
            [
                diagnostic(for: harness.global, client: harness.name, scope: .global),
                harness.project.map {
                    diagnostic(for: $0, client: harness.name, scope: .project)
                },
            ].compactMap { $0 }
        }
        return (
            MCPDiscoveryDiagnostics(servers: servers, configurations: configurations),
            ["mcp: source=symbrain (\(inventory.harnesses.count) harnesses)"]
        )
    }

    private static func diagnostic(
        for config: HarnessConfigInventory,
        client: String,
        scope: MCPConfigDiagnosticScope
    ) -> MCPConfigDiagnostic {
        let status: MCPConfigDiagnosticStatus
        let message: String?
        if !config.exists {
            status = .unavailable
            message = "Configuration is not present."
        } else if !config.parsed || !(config.error ?? "").isEmpty {
            status = .invalid
            message = "Brain could not parse this configuration."
        } else if config.servers.isEmpty {
            status = .empty
            message = "No MCP servers are configured."
        } else {
            status = .ready
            message = nil
        }
        return MCPConfigDiagnostic(
            client: client,
            configScope: scope,
            status: status,
            path: config.path,
            message: message
        )
    }

    private static func discoverViaSymbrain(_ inventory: HarnessInventory) -> ([MCPServer], [String]) {
        var servers: [MCPServer] = []
        var notes: [String] = ["mcp: source=symbrain (\(inventory.harnesses.count) harnesses)"]

        for harness in inventory.harnesses {
            for config in [harness.global, harness.project].compactMap({ $0 }) {
                if let error = config.error, !error.isEmpty {
                    notes.append("\(harness.name): \(error) in \(config.path)")
                    continue
                }
                guard config.parsed else { continue }
                for server in config.servers {
                    servers.append(
                        MCPServer(
                            name: server.name,
                            client: harness.name,
                            transport: server.transport,
                            command: server.command,
                            args: server.args,
                            url: server.url,
                            configPath: config.path,
                            secretBacked: !server.envNames.isEmpty,
                            credentialWarnings: server.envNames.map {
                                "requires environment variable \($0)"
                            },
                            source: "symbrain"
                        )
                    )
                }
            }
        }
        return (servers, notes)
    }
}
