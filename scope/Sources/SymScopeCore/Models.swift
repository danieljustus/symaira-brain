import Foundation

/// Data models for symscope — snake_case JSON, mirroring the Go contract
/// (symaira-scope internal/model). Structs are Codable with
/// `.convertToSnakeCase` at the encoder site.

public struct Snapshot: Codable, Sendable {
    public var generatedAt: String
    public var ports: [Port]
    public var mcpServers: [MCPServer]
    public var containers: [Container]
    public var daemons: [Daemon]
    public var notes: [String]

    public init(
        generatedAt: String = "",
        ports: [Port] = [],
        mcpServers: [MCPServer] = [],
        containers: [Container] = [],
        daemons: [Daemon] = [],
        notes: [String] = []
    ) {
        self.generatedAt = generatedAt
        self.ports = ports
        self.mcpServers = mcpServers
        self.containers = containers
        self.daemons = daemons
        self.notes = notes
    }

    public enum CodingKeys: String, CodingKey {
        case generatedAt = "generated_at"
        case ports
        case mcpServers = "mcp_servers"
        case containers
        case daemons
        case notes
    }
}

public struct Port: Codable, Equatable, Sendable {
    public var port: Int
    public var protocol_: String // tcp | udp
    public var address: String
    public var pid: Int
    public var process: String

    public init(port: Int, protocol_: String, address: String, pid: Int, process: String) {
        self.port = port
        self.protocol_ = protocol_
        self.address = address
        self.pid = pid
        self.process = process
    }

    public enum CodingKeys: String, CodingKey {
        case port
        case protocol_ = "protocol"
        case address
        case pid
        case process
    }
}

public struct MCPServer: Codable, Equatable, Sendable {
    public var name: String
    public var client: String // claude-desktop, cursor, vscode, ...
    public var transport: String // stdio | http | sse
    public var command: String?
    public var args: [String]
    public var url: String?
    public var configPath: String
    public var env: [String: String]
    public var secretBacked: Bool
    public var credentialWarnings: [String]
    /// Which inventory produced this entry: "symbrain" (the authoritative
    /// schema-2 harness inventory).
    public var source: String

    public init(
        name: String,
        client: String,
        transport: String,
        command: String? = nil,
        args: [String] = [],
        url: String? = nil,
        configPath: String,
        env: [String: String] = [:],
        secretBacked: Bool = false,
        credentialWarnings: [String] = [],
        source: String = "builtin"
    ) {
        self.name = name
        self.client = client
        self.transport = transport
        self.command = command
        self.args = args
        self.url = url
        self.configPath = configPath
        self.env = env
        self.secretBacked = secretBacked
        self.credentialWarnings = credentialWarnings
        self.source = source
    }

    public enum CodingKeys: String, CodingKey {
        case name
        case client
        case transport
        case command
        case args
        case url
        case configPath = "config_path"
        case env
        case secretBacked = "secret_backed"
        case credentialWarnings = "credential_warnings"
        case source
    }
}

public struct Container: Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var image: String
    public var ports: [Int]

    public init(id: String, name: String, image: String, ports: [Int] = []) {
        self.id = id
        self.name = name
        self.image = image
        self.ports = ports
    }
}

public struct MCPHealthResult: Codable, Equatable, Sendable {
    public var name: String
    public var client: String
    public var status: String // healthy | unhealthy | unknown
    public var latencyMs: Int64
    public var error: String?

    public init(name: String, client: String, status: String, latencyMs: Int64, error: String? = nil) {
        self.name = name
        self.client = client
        self.status = status
        self.latencyMs = latencyMs
        self.error = error
    }

    public enum CodingKeys: String, CodingKey {
        case name
        case client
        case status
        case latencyMs = "latency_ms"
        case error
    }
}

public struct Conflict: Codable, Equatable, Sendable {
    public var port: Int
    public var holders: [String]
    public var kind: String // process-process | mcp-occupied

    public init(port: Int, holders: [String], kind: String) {
        self.port = port
        self.holders = holders
        self.kind = kind
    }
}

public struct ClientConfig: Codable, Equatable, Sendable {
    public var client: String
    public var path: String
    public var present: Bool

    public init(client: String, path: String, present: Bool) {
        self.client = client
        self.path = path
        self.present = present
    }
}

public struct VersionInfo: Codable, Equatable, Sendable {
    public var tool: String
    public var version: String
    public var schemaVersion: Int

    public init(tool: String = "symscope", version: String, schemaVersion: Int = 1) {
        self.tool = tool
        self.version = version
        self.schemaVersion = schemaVersion
    }

    public enum CodingKeys: String, CodingKey {
        case tool
        case version
        case schemaVersion = "schema_version"
    }
}
