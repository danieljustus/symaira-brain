import Foundation

/// Errors raised when canonical history cannot be made into a safe replay.
public enum DeterministicReplayError: Error, Equatable, LocalizedError, Sendable {
    case invalidLimit
    case tooManyRecords(actual: Int, maximum: Int)
    case encodedSizeExceeded(actual: Int, maximum: Int)
    case unsupportedSchemaVersion(Int)
    case unsupportedSource(String)
    case unsupportedAction(String)
    case unsafeRecord(index: Int, reason: String)
    case nonDeterministicRecord(index: Int, reason: String)
    case malformedRecording

    public var errorDescription: String? {
        switch self {
        case .invalidLimit:
            return "Replay limits must be positive."
        case let .tooManyRecords(actual, maximum):
            return "Replay contains \(actual) records; the maximum is \(maximum)."
        case let .encodedSizeExceeded(actual, maximum):
            return "Replay is \(actual) bytes; the maximum is \(maximum) bytes."
        case let .unsupportedSchemaVersion(version):
            return "Replay schema version \(version) is not supported."
        case let .unsupportedSource(source):
            return "History source \"\(source)\" is not replayable."
        case let .unsupportedAction(action):
            return "Action \"\(action)\" is not replayable."
        case let .unsafeRecord(index, reason):
            return "Replay record \(index) is unsafe: \(reason)"
        case let .nonDeterministicRecord(index, reason):
            return "Replay record \(index) is non-deterministic: \(reason)"
        case .malformedRecording:
            return "The replay envelope is malformed."
        }
    }
}

/// A versioned, bounded sequence of privacy-sanitized canonical events.
///
/// `records` is intentionally an array rather than a set or dictionary. Its
/// order is the execution order and is preserved by both the model and codec.
public struct DeterministicReplayRecording: Codable, Equatable, Sendable {
    public static let currentSchemaVersion = 1

    public let schemaVersion: Int
    public let records: [CanonicalHistoryEvent]

    public init(
        records: [CanonicalHistoryEvent],
        schemaVersion: Int = DeterministicReplayRecording.currentSchemaVersion
    ) {
        self.schemaVersion = schemaVersion
        self.records = records
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case records
    }
}

/// Short aliases for clients that do not need the longer envelope name.
public typealias DeterministicReplay = DeterministicReplayRecording
public typealias ReplayRecording = DeterministicReplayRecording

/// Encodes canonical history as a deterministic, privacy-aware replay envelope.
///
/// This is future replay groundwork, not a live production boundary: the codec
/// currently has no production caller. Once a replay surface exists, it is
/// intended to be the safety boundary for that surface.
///
/// The codec is deliberately pure. It does not read files, consult the running
/// desktop, or post input. Replay execution must perform fresh target and
/// precondition checks before each returned event is acted upon.
public struct DeterministicReplayCodec: Sendable {
    public static let defaultMaximumRecords = 200
    public static let defaultMaximumBytes = 256 * 1024

    public let maximumRecords: Int
    public let maximumBytes: Int

    public init(
        maximumRecords: Int = DeterministicReplayCodec.defaultMaximumRecords,
        maximumBytes: Int = DeterministicReplayCodec.defaultMaximumBytes
    ) {
        self.maximumRecords = maximumRecords
        self.maximumBytes = maximumBytes
    }

    /// Builds a sanitized recording while retaining the source array order.
    public func recording(from records: [CanonicalHistoryEvent]) throws -> DeterministicReplayRecording {
        try validateLimits()
        guard records.count <= maximumRecords else {
            throw DeterministicReplayError.tooManyRecords(actual: records.count, maximum: maximumRecords)
        }

        let sanitized = try records.enumerated().map { index, record in
            try validate(record, at: index)
        }
        return DeterministicReplayRecording(records: sanitized)
    }

    /// Encodes records with sorted object keys and unchanged record ordering.
    public func encode(_ records: [CanonicalHistoryEvent]) throws -> Data {
        try encodeValidated(recording(from: records))
    }

    /// Encodes an already-built recording after applying the same validation.
    public func encode(_ recording: DeterministicReplayRecording) throws -> Data {
        let validated = try self.recording(from: recording.records)
        guard recording.schemaVersion == DeterministicReplayRecording.currentSchemaVersion else {
            throw DeterministicReplayError.unsupportedSchemaVersion(recording.schemaVersion)
        }
        return try encodeValidated(validated)
    }

    /// Encodes a recording that has already crossed the validation and redaction boundary.
    private func encodeValidated(_ recording: DeterministicReplayRecording) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(recording)
        guard data.count <= maximumBytes else {
            throw DeterministicReplayError.encodedSizeExceeded(actual: data.count, maximum: maximumBytes)
        }
        return data
    }

    /// Decodes and re-sanitizes an envelope so untrusted input cannot escape the
    /// privacy boundary merely by bypassing `encode`.
    public func decode(_ data: Data) throws -> DeterministicReplayRecording {
        try validateLimits()
        guard data.count <= maximumBytes else {
            throw DeterministicReplayError.encodedSizeExceeded(actual: data.count, maximum: maximumBytes)
        }

        let decoded: DeterministicReplayRecording
        do {
            decoded = try JSONDecoder().decode(DeterministicReplayRecording.self, from: data)
        } catch {
            throw DeterministicReplayError.malformedRecording
        }
        guard decoded.schemaVersion == DeterministicReplayRecording.currentSchemaVersion else {
            throw DeterministicReplayError.unsupportedSchemaVersion(decoded.schemaVersion)
        }
        return try recording(from: decoded.records)
    }

    public func decodeRecords(_ data: Data) throws -> [CanonicalHistoryEvent] {
        try decode(data).records
    }

    public static func encode(
        records: [CanonicalHistoryEvent],
        maximumRecords: Int = DeterministicReplayCodec.defaultMaximumRecords,
        maximumBytes: Int = DeterministicReplayCodec.defaultMaximumBytes
    ) throws -> Data {
        try DeterministicReplayCodec(maximumRecords: maximumRecords, maximumBytes: maximumBytes).encode(records)
    }

    public static func decode(
        _ data: Data,
        maximumRecords: Int = DeterministicReplayCodec.defaultMaximumRecords,
        maximumBytes: Int = DeterministicReplayCodec.defaultMaximumBytes
    ) throws -> DeterministicReplayRecording {
        try DeterministicReplayCodec(maximumRecords: maximumRecords, maximumBytes: maximumBytes).decode(data)
    }

    private static let replayableSources: Set<String> = ["operate"]
    private static let replayableActions: Set<String> = [
        "click", "delete", "drag", "focus_app", "menu_action", "press_keys", "scroll",
        "type_text"
    ]
    private static let stableTargetKeys: Set<String> = [
        "accessibility_id", "accessibility_identifier", "app_name", "bundle_id",
        "label", "role", "subrole", "title", "window_title"
    ]
    private static let ephemeralKeys: Set<String> = [
        "coordinates", "display_id", "element_id", "event_id", "from_element_id",
        "from_x", "from_y", "nonce", "pid", "process_id", "random", "request_id",
        "seed", "snapshot_id", "space_id", "to_element_id", "to_x", "to_y",
        "uuid", "window_id", "x", "y"
    ]
    private static let privateKeys: Set<String> = [
        "api_key", "authorization", "bearer", "card_number", "credential",
        "input_text", "one_time_code", "otp", "password", "private_key", "secret",
        "token"
    ]
    private static let privateKeyStems: Set<String> = ["auth", "cookie", "key", "session"]
    private static let destructiveWords: Set<String> = [
        "authorize", "delete", "erase", "force_quit", "keychain", "quit", "remove",
        "shutdown", "terminate", "unlock", "uninstall"
    ]

    private func validateLimits() throws {
        guard maximumRecords > 0, maximumBytes > 0 else {
            throw DeterministicReplayError.invalidLimit
        }
    }

    private func validate(_ event: CanonicalHistoryEvent, at index: Int) throws -> CanonicalHistoryEvent {
        guard event.schemaVersion == CanonicalHistoryEvent.currentSchemaVersion else {
            throw DeterministicReplayError.unsupportedSchemaVersion(event.schemaVersion)
        }

        let source = event.source.lowercased()
        guard Self.replayableSources.contains(source) else {
            throw DeterministicReplayError.unsupportedSource(event.source)
        }

        let action = event.action.lowercased()
        guard event.action == action, Self.replayableActions.contains(action) else {
            if Self.replayableActions.contains(action) {
                throw DeterministicReplayError.nonDeterministicRecord(index: index, reason: "action names must be canonical snake_case")
            }
            throw DeterministicReplayError.unsupportedAction(event.action)
        }

        guard !event.timestamp.isEmpty, Self.isISO8601(event.timestamp) else {
            throw DeterministicReplayError.nonDeterministicRecord(index: index, reason: "timestamp is not a fixed ISO-8601 value")
        }

        if case .bool(false) = event.payload["success"] {
            throw DeterministicReplayError.unsafeRecord(index: index, reason: "failed actions cannot be replayed")
        }
        if action == "type_text" {
            throw DeterministicReplayError.unsafeRecord(index: index, reason: "text input may contain secrets or personal content")
        }
        if containsDestructiveAction(in: .object(event.payload), action: action) {
            throw DeterministicReplayError.unsafeRecord(index: index, reason: "destructive controls are never replayed")
        }
        if let reason = nonDeterminismReason(in: .object(event.payload)) {
            throw DeterministicReplayError.nonDeterministicRecord(index: index, reason: reason)
        }
        guard hasStableTarget(in: event.payload) else {
            throw DeterministicReplayError.nonDeterministicRecord(index: index, reason: "a stable app/window or UI predicate target is required")
        }

        return CanonicalHistoryEvent(
            source: SecretRedactor.redact(event.source),
            timestamp: SecretRedactor.redact(event.timestamp),
            action: SecretRedactor.redact(event.action),
            payload: redactPayload(event.payload),
            schemaVersion: event.schemaVersion
        )
    }

    private static func isISO8601(_ value: String) -> Bool {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if formatter.date(from: value) != nil { return true }
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: value) != nil
    }

    private func hasStableTarget(in payload: [String: HistoryJSONValue]) -> Bool {
        let containers = ["replay_target", "target", "targets"]
        for key in containers {
            guard case .object(let target) = payload[key] else { continue }
            let keys = Set(target.keys.map(Self.normalizedKey))
            if !keys.intersection(Self.stableTargetKeys).isEmpty {
                return true
            }
        }
        return !Set(payload.keys.map(Self.normalizedKey)).intersection(Self.stableTargetKeys).isEmpty
    }

    private func nonDeterminismReason(in value: HistoryJSONValue, key: String? = nil) -> String? {
        if let key, Self.ephemeralKeys.contains(Self.normalizedKey(key)) {
            return "field \(key) is snapshot- or process-scoped"
        }
        switch value {
        case .object(let object):
            for (childKey, childValue) in object.sorted(by: { $0.key < $1.key }) {
                if let reason = nonDeterminismReason(in: childValue, key: childKey) { return reason }
            }
        case .array(let array):
            for child in array {
                if let reason = nonDeterminismReason(in: child) { return reason }
            }
        case .string(let text):
            if Self.isUUID(text) { return "UUID values are not stable replay identities" }
        default:
            break
        }
        return nil
    }

    private func containsDestructiveAction(in value: HistoryJSONValue, action: String) -> Bool {
        if Self.destructiveWords.contains(action) { return true }
        switch value {
        case .string(let text):
            let words = text.lowercased()
                .split(whereSeparator: { !$0.isLetter && !$0.isNumber && $0 != "_" })
                .map(String.init)
            return !Set(words).intersection(Self.destructiveWords).isEmpty
        case .object(let object):
            return object.contains { key, child in
                if Self.destructiveWords.contains(Self.normalizedKey(key)) { return true }
                return containsDestructiveAction(in: child, action: action)
            }
        case .array(let array):
            return array.contains { containsDestructiveAction(in: $0, action: action) }
        default:
            return false
        }
    }

    private func redactPayload(_ payload: [String: HistoryJSONValue]) -> [String: HistoryJSONValue] {
        var redacted: [String: HistoryJSONValue] = [:]
        for (entryKey, child) in payload {
            redacted[entryKey] = redactPayload(child, key: entryKey)
        }
        return redacted
    }

    private func redactPayload(_ value: HistoryJSONValue, key: String? = nil) -> HistoryJSONValue {
        if let key, Self.isPrivateKey(key) {
            return .string("<redacted>")
        }
        switch value {
        case .object(let object):
            var redacted: [String: HistoryJSONValue] = [:]
            for (entryKey, child) in object {
                redacted[entryKey] = redactPayload(child, key: entryKey)
            }
            return .object(redacted)
        case .array(let array):
            return .array(array.map { redactPayload($0) })
        default:
            return SecretRedactor.redact(value)
        }
    }

    private static func isPrivateKey(_ key: String) -> Bool {
        let normalized = normalizedKey(key)
        if privateKeys.contains(normalized) {
            return true
        }
        return normalized.split(separator: "_").contains { component in
            privateKeys.contains(String(component)) || privateKeyStems.contains(String(component))
        }
    }

    private static func normalizedKey(_ key: String) -> String {
        key.replacingOccurrences(of: "-", with: "_").lowercased()
    }

    private static func isUUID(_ value: String) -> Bool {
        let parts = value.split(separator: "-")
        return parts.count == 5 && [8, 4, 4, 4, 12].enumerated().allSatisfy { index, length in
            parts[index].count == length && parts[index].allSatisfy { $0.isHexDigit }
        }
    }
}

public typealias ReplayCodec = DeterministicReplayCodec
