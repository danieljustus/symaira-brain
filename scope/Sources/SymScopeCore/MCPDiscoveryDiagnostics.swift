import Foundation

/// Stable state for one configuration reported by Brain's harness inventory.
public enum MCPConfigDiagnosticStatus: String, Codable, Equatable, Sendable {
    case ready
    case empty
    case invalid
    case unavailable
}

/// The Brain-owned configuration variant represented by a diagnostic.
public enum MCPConfigDiagnosticScope: String, Codable, Equatable, Sendable {
    case global
    case project
}

/// A bounded, safe summary of one Brain-owned harness configuration.
public struct MCPConfigDiagnostic: Codable, Equatable, Sendable {
    public let client: String
    public let configScope: MCPConfigDiagnosticScope
    public let status: MCPConfigDiagnosticStatus
    public let path: String
    /// Generic state text only; Brain's free-form parser error is never echoed.
    public let message: String?

    public init(
        client: String,
        configScope: MCPConfigDiagnosticScope,
        status: MCPConfigDiagnosticStatus,
        path: String,
        message: String? = nil
    ) {
        self.client = client
        self.configScope = configScope
        self.status = status
        self.path = MCPDiagnosticPath.bounded(path)
        self.message = message
    }

    public enum CodingKeys: String, CodingKey {
        case client
        case configScope = "config_scope"
        case status
        case path
        case message
    }
}

/// Additive MCP discovery view; the existing server entries are unchanged.
public struct MCPDiscoveryDiagnostics: Codable, Equatable, Sendable {
    public let servers: [MCPServer]
    public let configurations: [MCPConfigDiagnostic]

    public init(servers: [MCPServer], configurations: [MCPConfigDiagnostic]) {
        self.servers = servers
        self.configurations = configurations
    }
}

enum MCPDiagnosticPath {
    static let maximumUTF8Length = 512

    static func bounded(_ path: String) -> String {
        let home = NSHomeDirectory()
        let displayPath: String
        if path == home {
            displayPath = "~"
        } else if path.hasPrefix(home + "/") {
            displayPath = "~" + path.dropFirst(home.count)
        } else {
            displayPath = path
        }

        var safe = String.UnicodeScalarView()
        for scalar in displayPath.unicodeScalars {
            safe.append(CharacterSet.controlCharacters.contains(scalar) ? "�" : scalar)
        }
        let value = String(safe)
        guard value.utf8.count > maximumUTF8Length else { return value }

        let suffix = "…"
        let prefixLimit = maximumUTF8Length - suffix.utf8.count
        var prefix = String.UnicodeScalarView()
        var bytes = 0
        for scalar in value.unicodeScalars {
            let scalarBytes = String(scalar).utf8.count
            guard bytes + scalarBytes <= prefixLimit else { break }
            prefix.append(scalar)
            bytes += scalarBytes
        }
        return String(prefix) + suffix
    }
}
