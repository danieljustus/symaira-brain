import Foundation

/// Explains what uses a port or which client owns an MCP server
/// (mirrors the Go original's explain command).
public enum ExplainService: Sendable {
    public struct PortExplanation: Codable, Equatable, Sendable {
        public var port: Int
        public var holders: [Port]
        public var mcpServers: [MCPServer]

        public init(port: Int, holders: [Port], mcpServers: [MCPServer]) {
            self.port = port
            self.holders = holders
            self.mcpServers = mcpServers
        }

        public enum CodingKeys: String, CodingKey {
            case port
            case holders
            case mcpServers = "mcp_servers"
        }
    }

    public struct ServerExplanation: Codable, Equatable, Sendable {
        public var name: String
        public var client: String
        public var configPath: String
        public var transport: String
        public var command: String?
        public var url: String?

        public init(name: String, client: String, configPath: String, transport: String, command: String?, url: String?) {
            self.name = name
            self.client = client
            self.configPath = configPath
            self.transport = transport
            self.command = command
            self.url = url
        }
    }

    /// Which processes hold a port, plus MCP servers whose command references it.
    public static func explainPort(_ port: Int, ports: [Port], servers: [MCPServer]) -> PortExplanation {
        let holders = ports.filter { $0.port == port }
        let mcpServers = servers.filter { server in
            server.args.contains { arg in
                arg.contains(":\(port)") || arg == "\(port)"
            }
        }
        return PortExplanation(port: port, holders: holders, mcpServers: mcpServers)
    }

    public static func explainServer(_ name: String, servers: [MCPServer]) -> ServerExplanation? {
        guard let server = servers.first(where: { $0.name == name }) else { return nil }
        return ServerExplanation(
            name: server.name,
            client: server.client,
            configPath: server.configPath,
            transport: server.transport,
            command: server.command,
            url: server.url
        )
    }
}
