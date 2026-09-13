import Foundation

/// One configured MCP server inside a symbrain harness inventory (schema 2).
public struct HarnessServerInventory: Codable, Equatable, Sendable {
    public let name: String
    public let transport: String
    public let command: String?
    public let args: [String]
    public let url: String?
    public let envNames: [String]

    public init(
        name: String,
        transport: String,
        command: String? = nil,
        args: [String] = [],
        url: String? = nil,
        envNames: [String] = []
    ) {
        self.name = name
        self.transport = transport
        self.command = command
        self.args = args
        self.url = url
        self.envNames = envNames
    }

    enum CodingKeys: String, CodingKey {
        case name
        case transport
        case command
        case args
        case url
        case envNames = "env_names"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        transport = try container.decode(String.self, forKey: .transport)
        command = try container.decodeIfPresent(String.self, forKey: .command)
        args = try container.decodeIfPresent([String].self, forKey: .args) ?? []
        url = try container.decodeIfPresent(String.self, forKey: .url)
        envNames = try container.decodeIfPresent([String].self, forKey: .envNames) ?? []
    }
}

/// One global or project-local harness config, as reported by symbrain.
public struct HarnessConfigInventory: Codable, Equatable, Sendable {
    public let path: String
    public let exists: Bool
    public let parsed: Bool
    public let error: String?
    public let servers: [HarnessServerInventory]

    public init(path: String, exists: Bool, parsed: Bool, error: String?, servers: [HarnessServerInventory]) {
        self.path = path
        self.exists = exists
        self.parsed = parsed
        self.error = error
        self.servers = servers
    }
}

/// One harness (e.g. "claude", "cursor", "codex") as reported by symbrain.
public struct HarnessInventoryEntry: Codable, Equatable, Sendable {
    public let name: String
    public let displayName: String
    public let global: HarnessConfigInventory
    public let project: HarnessConfigInventory?

    public init(name: String, displayName: String, global: HarnessConfigInventory, project: HarnessConfigInventory? = nil) {
        self.name = name
        self.displayName = displayName
        self.global = global
        self.project = project
    }

    enum CodingKeys: String, CodingKey {
        case name
        case displayName = "display_name"
        case global
        case project
    }
}

/// The full schema-2 inventory returned by `symbrain harness list --output json`.
public struct HarnessInventory: Codable, Equatable, Sendable {
    public let schemaVersion: Int
    public let projectDir: String?
    public let harnesses: [HarnessInventoryEntry]

    public init(schemaVersion: Int, projectDir: String?, harnesses: [HarnessInventoryEntry]) {
        self.schemaVersion = schemaVersion
        self.projectDir = projectDir
        self.harnesses = harnesses
    }

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case projectDir = "project_dir"
        case harnesses
    }
}

public struct HarnessHealthEntry: Codable, Equatable, Sendable {
    public let harness: String
    public let config: String
    public let server: String
    public let transport: String
    public let healthy: Bool
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case harness
        case config
        case server
        case transport
        case healthy
        case error
    }
}

public struct HarnessHealthReport: Codable, Equatable, Sendable {
    public let servers: [HarnessHealthEntry]

    enum CodingKeys: String, CodingKey {
        case servers
    }
}

/// symbrain's harness inventory is the SSOT for MCP servers. Cockpit only
/// presents this data and never reparses harness configuration files.
public protocol HarnessInventoryProviding: Sendable {
    var isAvailable: Bool { get }
    func list(projectDir: String?) -> HarnessInventory?
}

/// Production `symbrain` subprocess wrapper. Missing or failed symbrain is
/// surfaced to callers so they can report an actionable requirement.
public final class SymBrainHarnessService: HarnessInventoryProviding, @unchecked Sendable {
    public init() {}

    public var isAvailable: Bool {
        resolveSymbrain() != nil
    }

    public func list(projectDir: String?) -> HarnessInventory? {
        guard let path = resolveSymbrain() else { return nil }
        var arguments = ["harness", "list", "--output", "json"]
        if let projectDir {
            arguments += ["--project", projectDir]
        }
        // Truncated output is a prefix of a JSON document, so decoding it can
        // only produce a partial or wrong inventory. Treat it as a decode
        // failure so the caller reports its actionable note instead.
        guard let result = try? BoundedProcessRunner.run(executable: path, arguments: arguments),
              !result.timedOut,
              !result.truncated,
              result.terminationStatus == 0 else {
            return nil
        }
        return try? JSONDecoder().decode(HarnessInventory.self, from: result.standardOutput)
    }

    /// Returns the health states produced by symbrain's bounded, concurrent
    /// harness probes, converted to Cockpit's stable MCP health shape.
    public func health() -> [MCPHealthResult]? {
        guard let path = resolveSymbrain(),
              let result = try? BoundedProcessRunner.run(
                  executable: path,
                  arguments: ["harness", "health", "--json"],
                  timeoutSeconds: 30
              ),
              !result.timedOut,
              !result.truncated,
              result.terminationStatus == 0,
              let report = try? JSONDecoder().decode(HarnessHealthReport.self, from: result.standardOutput) else {
            return nil
        }
        return report.servers.map {
            MCPHealthResult(
                name: $0.server,
                client: $0.harness,
                status: $0.healthy ? "healthy" : "unhealthy",
                latencyMs: 0,
                error: $0.error
            )
        }
    }

    private func resolveSymbrain() -> String? {
        Self.resolveSymbrain(
            environment: ProcessInfo.processInfo.environment,
            isExecutableFile: { FileManager.default.isExecutableFile(atPath: $0) },
            pathLookup: Self.lookPath
        )
    }

    static func resolveSymbrain(
        environment: [String: String],
        isExecutableFile: (String) -> Bool,
        pathLookup: (String) -> String?
    ) -> String? {
        if let bin = environment["SYMAIRA_BIN"] {
            let candidate = "\(bin)/symbrain"
            if isExecutableFile(candidate) {
                return candidate
            }
        }
        if let home = environment["HOME"] {
            let managed = "\(home)/.symaira/bin/symbrain"
            if isExecutableFile(managed) {
                return managed
            }
        }
        if let found = pathLookup("symbrain") {
            return found
        }
        // A GUI-launched process inherits launchd's minimal PATH
        // (/usr/bin:/bin:/usr/sbin:/sbin), which never includes Homebrew's
        // prefix — only an interactive shell's .zprofile/.zshrc adds that.
        // Without this fallback, a symbrain installed exactly the way the
        // app's own error note recommends (`brew install
        // danieljustus/tap/symbrain`) is invisible from the cockpit GUI even
        // though `symbrain harness list` works fine from a terminal.
        // Mirrors ContainerService.resolveDockerBinary()'s fixed candidate list.
        for candidate in ["/opt/homebrew/bin/symbrain", "/usr/local/bin/symbrain"] {
            if isExecutableFile(candidate) {
                return candidate
            }
        }
        return nil
    }

    private static func lookPath(_ name: String) -> String? {
        guard let pathEnv = ProcessInfo.processInfo.environment["PATH"] else { return nil }
        for dir in pathEnv.split(separator: ":", omittingEmptySubsequences: true) {
            let candidate = "\(dir)/\(name)"
            if FileManager.default.isExecutableFile(atPath: candidate) {
                return candidate
            }
        }
        return nil
    }
}
