import Foundation

/// JSON values used for component-specific fields in the canonical history envelope.
public enum HistoryJSONValue: Codable, Equatable, Sendable {
    case string(String)
    case number(Double)
    case integer(Int64)
    case bool(Bool)
    case object([String: HistoryJSONValue])
    case array([HistoryJSONValue])
    case null

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Int64.self) {
            self = .integer(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([String: HistoryJSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([HistoryJSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported JSON value")
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .integer(let value): try container.encode(value)
        case .bool(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .null: try container.encodeNil()
        }
    }

    public static func fromJSONData(_ data: Data) throws -> HistoryJSONValue {
        let object = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        return try fromFoundation(object)
    }

    public func jsonData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(self)
    }

    private static func fromFoundation(_ value: Any) throws -> HistoryJSONValue {
        if value is NSNull { return .null }
        if let value = value as? String { return .string(value) }
        if let value = value as? NSNumber {
            if CFGetTypeID(value) == CFBooleanGetTypeID() { return .bool(value.boolValue) }
            if value.doubleValue.rounded() == value.doubleValue,
               value.doubleValue >= Double(Int64.min), value.doubleValue <= Double(Int64.max) {
                return .integer(value.int64Value)
            }
            return .number(value.doubleValue)
        }
        if let value = value as? [Any] {
            return .array(try value.map(fromFoundation))
        }
        if let value = value as? [String: Any] {
            return .object(try value.mapValues(fromFoundation))
        }
        throw CocoaError(.propertyListReadCorrupt)
    }

    public var stringValue: String? {
        guard case .string(let value) = self else { return nil }
        return value
    }
}

/// Versioned, append-only history envelope shared by all symcockpit families.
public struct CanonicalHistoryEvent: Codable, Equatable, Sendable {
    public static let currentSchemaVersion = 1

    public let schemaVersion: Int
    public let source: String
    public let timestamp: String
    public let action: String
    public let payload: [String: HistoryJSONValue]

    public init(
        source: String,
        timestamp: String,
        action: String,
        payload: [String: HistoryJSONValue] = [:],
        schemaVersion: Int = CanonicalHistoryEvent.currentSchemaVersion
    ) {
        self.schemaVersion = schemaVersion
        self.source = source
        self.timestamp = timestamp
        self.action = action
        self.payload = payload
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case source
        case timestamp
        case action
        case payload
    }

    fileprivate func redacted() -> CanonicalHistoryEvent {
        CanonicalHistoryEvent(
            source: SecretRedactor.redact(source),
            timestamp: SecretRedactor.redact(timestamp),
            action: SecretRedactor.redact(action),
            payload: payload.mapValues { SecretRedactor.redact($0) },
            schemaVersion: schemaVersion
        )
    }
}

public typealias HistoryRecord = CanonicalHistoryEvent


/// The sole JSONL writer/reader used by tune and operate.
public final class CanonicalHistoryStore: @unchecked Sendable {
    public let fileURL: URL
    private let maxEvents: Int
    private let lock = NSLock()

    public init(fileURL: URL, maxEvents: Int = 1000) {
        self.fileURL = fileURL
        self.maxEvents = max(1, maxEvents)
        let manager = FileManager.default
        try? manager.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        try? manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fileURL.deletingLastPathComponent().path)
        if !manager.fileExists(atPath: fileURL.path) {
            manager.createFile(atPath: fileURL.path, contents: nil, attributes: [.posixPermissions: 0o600])
        }
        secureFile()
    }

    public var isWritable: Bool {
        let manager = FileManager.default
        if manager.fileExists(atPath: fileURL.path) { return manager.isWritableFile(atPath: fileURL.path) }
        return manager.isWritableFile(atPath: fileURL.deletingLastPathComponent().path)
    }

    public func append(_ event: CanonicalHistoryEvent) throws {
        lock.lock()
        defer { lock.unlock() }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(event.redacted())
        var line = data
        line.append(0x0A)
        let handle = try FileHandle(forWritingTo: fileURL)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: line)
        try trimIfNeeded()
        secureFile()
    }

    /// Reads canonical records and migrates the two legacy flat JSON shapes in memory.
    /// Invalid and unknown lines are ignored so one corrupt append cannot hide later data.
    public func read(limit: Int? = nil) throws -> [CanonicalHistoryEvent] {
        lock.lock()
        defer { lock.unlock() }
        guard let data = try? Data(contentsOf: fileURL),
              let contents = String(data: data, encoding: .utf8) else { return [] }
        var records: [CanonicalHistoryEvent] = []
        for line in contents.split(whereSeparator: \.isNewline) {
            guard let data = String(line).data(using: .utf8) else { continue }
            if let event = try? JSONDecoder().decode(CanonicalHistoryEvent.self, from: data),
               event.schemaVersion <= CanonicalHistoryEvent.currentSchemaVersion {
                records.append(event)
                continue
            }
            guard let value = try? HistoryJSONValue.fromJSONData(data),
                  case .object(let payload) = value,
                  let action = payload["action"]?.stringValue,
                  let timestamp = payload["timestamp"]?.stringValue else { continue }
            records.append(CanonicalHistoryEvent(source: "legacy", timestamp: timestamp, action: action, payload: payload))
        }
        if let limit, limit > 0 { return Array(records.suffix(limit)) }
        return records
    }

    private func trimIfNeeded() throws {
        let contents = try String(contentsOf: fileURL, encoding: .utf8)
        let lines = contents.split(whereSeparator: \.isNewline)
        guard lines.count > maxEvents else { return }
        let trimmed = lines.suffix(maxEvents).map(String.init).joined(separator: "\n") + "\n"
        try trimmed.write(to: fileURL, atomically: true, encoding: .utf8)
    }

    private func secureFile() {
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: fileURL.path)
    }
}
