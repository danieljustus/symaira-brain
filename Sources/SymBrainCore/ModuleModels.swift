// ModuleModels — Decodable types for the embedded Memory and Vault modules.
//
// These match the JSON emitted by `symmemory -o json ...` and
// `symvault --output json ...`.  Unlike the symbrain models they are decoded
// with a plain JSONDecoder and explicit snake_case CodingKeys: both tools
// return free-form dictionaries (memory metadata, vault fields) whose keys
// must survive verbatim, and `.convertFromSnakeCase` would rewrite those keys
// too ("api_key" -> "apiKey").

import Foundation

// MARK: - JSONValue

/// A loosely typed JSON value, used for free-form dictionaries where the CLI
/// does not promise a fixed value type (memory metadata, vault fields).
public enum JSONValue: Decodable, Sendable, Equatable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: JSONValue])
    case array([JSONValue])
    case null

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.dataCorruptedError(
                in: container,
                debugDescription: "Unsupported JSON value"
            )
        }
    }

    /// A single-line-ish rendering for table cells and detail rows.
    public var displayString: String {
        switch self {
        case .string(let value): value
        case .number(let value):
            value == value.rounded() && abs(value) < 1e15 ? String(Int(value)) : String(value)
        case .bool(let value): value ? "true" : "false"
        case .object(let value):
            value.keys.sorted()
                .map { "\($0): \(value[$0]?.displayString ?? "")" }
                .joined(separator: "\n")
        case .array(let value): value.map(\.displayString).joined(separator: ", ")
        case .null: "—"
        }
    }

    public var isEmpty: Bool {
        displayString.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

// MARK: - Timestamps

/// Formats an RFC3339 timestamp emitted by the Go CLIs for display in local
/// time.  Both tools mix fractional-second and offset variants, so several
/// parse options are attempted before falling back to the raw string.
public func formatModuleTimestamp(_ raw: String) -> String {
    guard let date = parseModuleTimestamp(raw) else { return raw }
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
    formatter.timeZone = .current
    return formatter.string(from: date)
}

/// Parses an RFC3339 timestamp with or without fractional seconds.
public func parseModuleTimestamp(_ raw: String) -> Date? {
    let withFraction = ISO8601DateFormatter()
    withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    if let date = withFraction.date(from: raw) { return date }

    let plain = ISO8601DateFormatter()
    plain.formatOptions = [.withInternetDateTime]
    return plain.date(from: raw)
}

// MARK: - Text reports

/// Picks the text to show for a CLI command that has no JSON mode.
///
/// stdout wins whenever it carries content: commands like `doctor` exit
/// non-zero on a failing check and print their usage text to stderr, which
/// would bury the report itself.
func reportText(stdout: String, stderr: String) -> String {
    let out = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
    if !out.isEmpty { return out }
    let err = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
    return err.isEmpty ? "No output." : err
}

// MARK: - Memory records

/// One entry from `symmemory list -o json` / `symmemory get <id> -o json`.
public struct MemoryRecord: Decodable, Sendable, Identifiable, Equatable {
    public let id: String
    public let content: String
    public let scope: String
    public let metadata: [String: JSONValue]?
    public let createdAt: String
    public let updatedAt: String?
    public let createdBy: String?
    public let consolidationStatus: String?
    public let reviewStatus: String?
    public let tier: String?
    public let importance: Double?
    public let decayFactor: Double?
    public let accessCount: Int?
    public let expiresAt: String?

    enum CodingKeys: String, CodingKey {
        case id
        case content
        case scope
        case metadata
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case createdBy = "created_by"
        case consolidationStatus = "consolidation_status"
        case reviewStatus = "review_status"
        case tier
        case importance
        case decayFactor = "decay_factor"
        case accessCount = "access_count"
        case expiresAt = "expires_at"
    }

    public var formattedCreatedAt: String { formatModuleTimestamp(createdAt) }

    /// The tier reported by the CLI, normalised for display: older records
    /// were written before tiers existed and report an empty string.
    public var displayTier: String {
        guard let tier, !tier.isEmpty else { return "untiered" }
        return tier
    }

    /// Metadata rendered as sorted key/value pairs for the detail panel.
    public var metadataPairs: [(key: String, value: String)] {
        guard let metadata else { return [] }
        return metadata.keys.sorted().compactMap { key in
            guard let value = metadata[key], !value.isEmpty else { return nil }
            return (key, value.displayString)
        }
    }
}

/// One hit from `symmemory search <query> -o json`.
public struct MemorySearchHit: Decodable, Sendable, Identifiable, Equatable {
    public let memory: MemoryRecord
    public let similarityScore: Double?

    enum CodingKeys: String, CodingKey {
        case memory
        case similarityScore = "similarity_score"
    }

    public var id: String { memory.id }
}

/// One entry from `symmemory rule list -o json`.
public struct MemoryRule: Decodable, Sendable, Identifiable, Equatable {
    public let id: String
    public let content: String
    public let scope: String
    public let metadata: [String: JSONValue]?
    public let createdAt: String
    public let updatedAt: String?

    enum CodingKeys: String, CodingKey {
        case id
        case content
        case scope
        case metadata
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }

    public var formattedCreatedAt: String { formatModuleTimestamp(createdAt) }
}

/// Result of `symmemory query-log --json` — the MCP tool-call log.
public struct MemoryQueryLog: Decodable, Sendable, Equatable {
    public let totalQueries: Int
    public let toolBreakdown: [String: Int]?
    public let actorBreakdown: [String: Int]?
    public let recentEntries: [MemoryQueryLogEntry]?

    enum CodingKeys: String, CodingKey {
        case totalQueries = "total_queries"
        case toolBreakdown = "tool_breakdown"
        case actorBreakdown = "actor_breakdown"
        case recentEntries = "recent_entries"
    }

    public var entries: [MemoryQueryLogEntry] { recentEntries ?? [] }

    /// Tool counts sorted by frequency, then name — stable ordering for the UI.
    public var toolCounts: [(name: String, count: Int)] {
        sortedCounts(toolBreakdown)
    }

    public var actorCounts: [(name: String, count: Int)] {
        sortedCounts(actorBreakdown)
    }

    private func sortedCounts(_ source: [String: Int]?) -> [(name: String, count: Int)] {
        guard let source else { return [] }
        return source
            .map { (name: $0.key, count: $0.value) }
            .sorted { lhs, rhs in
                lhs.count == rhs.count ? lhs.name < rhs.name : lhs.count > rhs.count
            }
    }
}

/// One recorded MCP query from `symmemory query-log --json`.
public struct MemoryQueryLogEntry: Decodable, Sendable, Identifiable, Equatable {
    public let id: String
    public let actor: String?
    public let tool: String
    public let queryText: String?
    public let params: String?
    public let durationMs: Int64?
    public let createdAt: String

    enum CodingKeys: String, CodingKey {
        case id
        case actor
        case tool
        case queryText = "query_text"
        case params
        case durationMs = "duration_ms"
        case createdAt = "created_at"
    }

    public var formattedCreatedAt: String { formatModuleTimestamp(createdAt) }
}

// MARK: - Vault entries

/// One entry from `symvault list --output json`.
public struct VaultEntrySummary: Decodable, Sendable, Identifiable, Equatable {
    public let path: String
    public let type: String?
    public let usageHint: String?
    public let autoRotate: Bool?
    public let hasValue: Bool?
    public let fieldCount: Int?

    enum CodingKeys: String, CodingKey {
        case path
        case type
        case usageHint = "usage_hint"
        case autoRotate = "auto_rotate"
        case hasValue = "has_value"
        case fieldCount = "field_count"
    }

    public var id: String { path }

    /// Last path component — the entry name shown in the list.
    public var title: String {
        path.split(separator: "/").last.map(String.init) ?? path
    }

    /// First path component — the folder the entry is grouped under.
    public var group: String {
        let parts = path.split(separator: "/")
        return parts.count > 1 ? String(parts[0]) : "General"
    }
}

/// The TOTP block of `symvault get <path> --output json`.
public struct VaultTOTP: Decodable, Sendable, Equatable {
    public let code: String
    public let period: Int?
    public let remaining: Int?
}

/// Result of `symvault get <path> --output json`.
///
/// The CLI has emitted both lower- and upper-cased top-level keys across
/// versions, so both spellings are accepted.
public struct VaultEntryDetail: Decodable, Sendable, Equatable {
    public let path: String
    public let modified: String?
    public let fields: [String: JSONValue]
    public let totp: VaultTOTP?

    private struct AnyKey: CodingKey {
        let stringValue: String
        let intValue: Int? = nil
        init(_ stringValue: String) { self.stringValue = stringValue }
        init?(stringValue: String) { self.init(stringValue) }
        init?(intValue: Int) { nil }
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: AnyKey.self)
        func value<T: Decodable>(_ type: T.Type, _ lower: String, _ upper: String) throws -> T? {
            if let found = try container.decodeIfPresent(type, forKey: AnyKey(lower)) {
                return found
            }
            return try container.decodeIfPresent(type, forKey: AnyKey(upper))
        }
        path = try value(String.self, "path", "Path") ?? ""
        modified = try value(String.self, "modified", "Modified")
        fields = try value([String: JSONValue].self, "fields", "Fields") ?? [:]
        totp = try value(VaultTOTP.self, "totp", "TOTP")
    }

    public init(
        path: String,
        modified: String?,
        fields: [String: JSONValue],
        totp: VaultTOTP? = nil
    ) {
        self.path = path
        self.modified = modified
        self.fields = fields
        self.totp = totp
    }

    /// The entry's primary secret, if one of the known secret fields is set.
    public var primarySecret: (field: String, value: String)? {
        for key in ["password", "secret", "token", "api_key", "private_key", "database_url"] {
            if let value = fields[key]?.displayString, !value.isEmpty {
                return (key, value)
            }
        }
        return nil
    }

    /// Field rows sorted with the secret first, then alphabetically.
    public var sortedFields: [(key: String, value: String, isSensitive: Bool)] {
        let secretField = primarySecret?.field
        return fields.keys.sorted { lhs, rhs in
            if lhs == secretField { return true }
            if rhs == secretField { return false }
            return lhs < rhs
        }
        .compactMap { key in
            guard let value = fields[key], !value.isEmpty else { return nil }
            return (key, value.displayString, VaultFieldSecurity.isSensitive(key))
        }
    }

    public var formattedModified: String? {
        modified.map(formatModuleTimestamp)
    }
}

/// Classifies vault field names that must stay masked until revealed.
public enum VaultFieldSecurity {
    public static func isSensitive(_ field: String) -> Bool {
        let key = field.lowercased()
        return [
            "password", "secret", "token", "api_key", "private_key", "totp",
            "certificate", "database_url",
        ]
        .contains { key.contains($0) }
    }
}
