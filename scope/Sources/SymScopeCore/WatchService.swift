import Foundation

/// Detects changes between two snapshots (mirrors the Go original's
/// internal/watch). Event types: port_bound, port_unbound, conflict_detected,
/// conflict_changed, conflict_resolved, mcp_server_added, mcp_server_changed,
/// mcp_server_removed, container_started, container_stopped, container_changed.
public struct WatchEvent: Codable, Equatable, Sendable {
    public var type: String
    public var timestamp: String
    public var payload: WatchPayload

    public init(type: String, timestamp: String, payload: WatchPayload) {
        self.type = type
        self.timestamp = timestamp
        self.payload = payload
    }
}

/// Payload is one of the model types; encoded inline in the event.
public enum WatchPayload: Codable, Equatable, Sendable {
    case port(Port)
    case conflict(Conflict)
    case mcpServer(MCPServer)
    case container(Container)

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let p = try? container.decode(Port.self) {
            self = .port(p)
        } else if let c = try? container.decode(Conflict.self) {
            self = .conflict(c)
        } else if let s = try? container.decode(MCPServer.self) {
            self = .mcpServer(s)
        } else if let c = try? container.decode(Container.self) {
            self = .container(c)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "unknown payload")
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .port(let p): try container.encode(p)
        case .conflict(let c): try container.encode(c)
        case .mcpServer(let s): try container.encode(s)
        case .container(let c): try container.encode(c)
        }
    }
}

public enum WatchService: Sendable {
    public static func diff(old: Snapshot, new: Snapshot) -> [WatchEvent] {
        var events: [WatchEvent] = []
        let now = ISO8601DateFormatter().string(from: Date())

        // Ports
        let oldPorts = Dictionary(uniqueKeysWithValues: old.ports.map { (portKey($0), $0) })
        let newPorts = Dictionary(uniqueKeysWithValues: new.ports.map { (portKey($0), $0) })
        for (key, np) in newPorts where oldPorts[key] == nil {
            events.append(WatchEvent(type: "port_bound", timestamp: now, payload: .port(np)))
        }
        for (key, op) in oldPorts where newPorts[key] == nil {
            events.append(WatchEvent(type: "port_unbound", timestamp: now, payload: .port(op)))
        }

        // Conflicts
        let oldConflicts = conflictMap(old.ports)
        let newConflicts = conflictMap(new.ports)
        for (key, nc) in newConflicts {
            if let oc = oldConflicts[key] {
                if nc.holders != oc.holders {
                    events.append(WatchEvent(type: "conflict_changed", timestamp: now, payload: .conflict(nc)))
                }
            } else {
                events.append(WatchEvent(type: "conflict_detected", timestamp: now, payload: .conflict(nc)))
            }
        }
        for (key, oc) in oldConflicts where newConflicts[key] == nil {
            events.append(WatchEvent(type: "conflict_resolved", timestamp: now, payload: .conflict(oc)))
        }

        // MCP servers
        let oldMCP = Dictionary(uniqueKeysWithValues: old.mcpServers.map { (mcpKey($0), $0) })
        let newMCP = Dictionary(uniqueKeysWithValues: new.mcpServers.map { (mcpKey($0), $0) })
        for (key, ns) in newMCP {
            if let os = oldMCP[key] {
                if ns != os {
                    events.append(WatchEvent(type: "mcp_server_changed", timestamp: now, payload: .mcpServer(ns)))
                }
            } else {
                events.append(WatchEvent(type: "mcp_server_added", timestamp: now, payload: .mcpServer(ns)))
            }
        }
        for (key, os) in oldMCP where newMCP[key] == nil {
            events.append(WatchEvent(type: "mcp_server_removed", timestamp: now, payload: .mcpServer(os)))
        }

        // Containers
        let oldCont = Dictionary(uniqueKeysWithValues: old.containers.map { ($0.id, $0) })
        let newCont = Dictionary(uniqueKeysWithValues: new.containers.map { ($0.id, $0) })
        for (key, nc) in newCont {
            if let oc = oldCont[key] {
                if nc != oc {
                    events.append(WatchEvent(type: "container_changed", timestamp: now, payload: .container(nc)))
                }
            } else {
                events.append(WatchEvent(type: "container_started", timestamp: now, payload: .container(nc)))
            }
        }
        for (key, oc) in oldCont where newCont[key] == nil {
            events.append(WatchEvent(type: "container_stopped", timestamp: now, payload: .container(oc)))
        }

        return events
    }

    // MARK: - Keys

    static func portKey(_ p: Port) -> String { "\(p.port)/\(p.protocol_)/\(p.pid)/\(p.address)" }

    static func mcpKey(_ s: MCPServer) -> String { "\(s.client)/\(s.name)" }

    static func conflictKey(_ c: Conflict) -> String { "\(c.port)/\(c.kind)" }

    static func conflictMap(_ ports: [Port]) -> [String: Conflict] {
        var result: [String: Conflict] = [:]
        for conflict in ConflictDetector.detect(ports) {
            result[conflictKey(conflict)] = conflict
        }
        return result
    }
}
