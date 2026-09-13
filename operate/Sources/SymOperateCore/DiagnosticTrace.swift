import Foundation
import SymCockpitHistory

/// Versioned limits for locally persisted diagnostic evidence.
public enum DiagnosticTraceContract {
    public static let currentVersion = 1
    public static let maxRecords = 64
    public static let maxBytes = 128 * 1024
    public static let maxStringLength = 2_048
}

/// Bounded, screenshot-free evidence captured before or after an action.
public struct DiagnosticTraceObservation: Codable, Sendable, Equatable {
    public let snapshotID: String?
    public let nodeCount: Int?

    public init(snapshotID: String? = nil, nodeCount: Int? = nil) {
        self.snapshotID = snapshotID.map(DiagnosticTraceRedactor.redact)
        self.nodeCount = nodeCount.map { max(0, $0) }
    }

    fileprivate func resanitized() -> Self {
        Self(snapshotID: snapshotID, nodeCount: nodeCount)
    }
}

/// The policy decision and effective permissions at the time of an action.
public struct DiagnosticTracePolicy: Codable, Sendable, Equatable {
    public let decision: String
    public let permissions: [String]

    public init(decision: String, permissions: [String] = []) {
        self.decision = DiagnosticTraceRedactor.redact(decision)
        self.permissions = Array(Set(permissions.map(DiagnosticTraceRedactor.redact))).sorted()
    }

    fileprivate func resanitized() -> Self {
        Self(decision: decision, permissions: permissions)
    }
}

/// A classified action result suitable for local diagnosis.
public struct DiagnosticTraceOutcome: Codable, Sendable, Equatable {
    public let success: Bool
    public let effect: EffectState
    public let verification: ActionVerification
    public let message: String
    public let errorCode: String?

    public init(
        success: Bool,
        effect: EffectState,
        verification: ActionVerification,
        message: String,
        errorCode: String? = nil
    ) {
        self.success = success
        self.effect = effect
        self.verification = verification
        self.message = DiagnosticTraceRedactor.redact(message)
        self.errorCode = errorCode.map(DiagnosticTraceRedactor.redact)
    }

    fileprivate func resanitized() -> Self {
        Self(
            success: success,
            effect: effect,
            verification: verification,
            message: message,
            errorCode: errorCode
        )
    }
}

/// A complete, correlated and bounded trace for one automation action.
public struct DiagnosticTraceRecord: Codable, Sendable, Equatable {
    public let schemaVersion: Int
    public let traceID: String
    public let action: String
    public let startedAt: String
    public let finishedAt: String
    public let request: [String: String]
    public let target: ActionTarget?
    public let before: DiagnosticTraceObservation?
    public let after: DiagnosticTraceObservation?
    public let route: ActionRouteDiagnostics?
    public let policy: DiagnosticTracePolicy
    public let outcome: DiagnosticTraceOutcome

    private enum CodingKeys: String, CodingKey {
        case schemaVersion
        case traceID
        case action
        case startedAt
        case finishedAt
        case request
        case target
        case before
        case after
        case route
        case policy
        case outcome
    }

    private enum DecodingKeys: String, CodingKey {
        case schemaVersion
        case traceID
        case traceId
        case action
        case startedAt
        case finishedAt
        case request
        case target
        case before
        case after
        case route
        case policy
        case outcome
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(schemaVersion, forKey: .schemaVersion)
        try container.encode(traceID, forKey: .traceID)
        try container.encode(action, forKey: .action)
        try container.encode(startedAt, forKey: .startedAt)
        try container.encode(finishedAt, forKey: .finishedAt)
        try container.encode(request, forKey: .request)
        try container.encodeIfPresent(target, forKey: .target)
        try container.encodeIfPresent(before, forKey: .before)
        try container.encodeIfPresent(after, forKey: .after)
        try container.encodeIfPresent(route, forKey: .route)
        try container.encode(policy, forKey: .policy)
        try container.encode(outcome, forKey: .outcome)
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        schemaVersion = try container.decode(Int.self, forKey: .schemaVersion)
        traceID = try container.decodeIfPresent(String.self, forKey: .traceID)
            ?? container.decode(String.self, forKey: .traceId)
        action = try container.decode(String.self, forKey: .action)
        startedAt = try container.decode(String.self, forKey: .startedAt)
        finishedAt = try container.decode(String.self, forKey: .finishedAt)
        request = try container.decode([String: String].self, forKey: .request)
        target = try container.decodeIfPresent(ActionTarget.self, forKey: .target)
        before = try container.decodeIfPresent(DiagnosticTraceObservation.self, forKey: .before)
        after = try container.decodeIfPresent(DiagnosticTraceObservation.self, forKey: .after)
        route = try container.decodeIfPresent(ActionRouteDiagnostics.self, forKey: .route)
        policy = try container.decode(DiagnosticTracePolicy.self, forKey: .policy)
        outcome = try container.decode(DiagnosticTraceOutcome.self, forKey: .outcome)
    }

    public init(
        traceID: String,
        action: String,
        startedAt: String,
        finishedAt: String,
        request: [String: String] = [:],
        target: ActionTarget? = nil,
        before: DiagnosticTraceObservation? = nil,
        after: DiagnosticTraceObservation? = nil,
        route: ActionRouteDiagnostics? = nil,
        policy: DiagnosticTracePolicy = DiagnosticTracePolicy(decision: "unknown"),
        outcome: DiagnosticTraceOutcome
    ) {
        self.schemaVersion = DiagnosticTraceContract.currentVersion
        self.traceID = DiagnosticTraceRedactor.redact(traceID)
        self.action = DiagnosticTraceRedactor.redact(action)
        self.startedAt = DiagnosticTraceRedactor.redact(startedAt)
        self.finishedAt = DiagnosticTraceRedactor.redact(finishedAt)
        self.request = DiagnosticTraceRedactor.redact(request)
        self.target = target
        self.before = before
        self.after = after
        self.route = route
        self.policy = policy
        self.outcome = outcome
    }

    private init(alreadySanitized trace: DiagnosticTraceRecord) {
        self.schemaVersion = trace.schemaVersion
        self.traceID = trace.traceID
        self.action = trace.action
        self.startedAt = trace.startedAt
        self.finishedAt = trace.finishedAt
        self.request = trace.request
        self.target = trace.target
        self.before = trace.before
        self.after = trace.after
        self.route = trace.route
        self.policy = trace.policy
        self.outcome = trace.outcome
    }

    /// Copies a record that has already crossed the construction boundary.
    fileprivate func sanitized() -> DiagnosticTraceRecord {
        DiagnosticTraceRecord(alreadySanitized: self)
    }

    /// Re-sanitizes a record decoded from an untrusted envelope.
    fileprivate func resanitized() -> DiagnosticTraceRecord {
        DiagnosticTraceRecord(
            traceID: traceID,
            action: action,
            startedAt: startedAt,
            finishedAt: finishedAt,
            request: request,
            target: target,
            before: before?.resanitized(),
            after: after?.resanitized(),
            route: route,
            policy: policy.resanitized(),
            outcome: outcome.resanitized()
        )
    }
}

/// Codable serializer for deterministic, privacy-aware local export.
public struct DiagnosticTraceSerializer: Sendable {
    public let maximumRecords: Int
    public let maximumBytes: Int

    public init(
        maximumRecords: Int = DiagnosticTraceContract.maxRecords,
        maximumBytes: Int = DiagnosticTraceContract.maxBytes
    ) {
        self.maximumRecords = maximumRecords
        self.maximumBytes = maximumBytes
    }

    public func encode(_ trace: DiagnosticTraceRecord) throws -> Data {
        guard maximumRecords > 0, maximumBytes > 0 else { throw DiagnosticTraceError.invalidLimits }
        let sanitized = trace.sanitized()
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(sanitized)
        guard data.count <= maximumBytes else {
            throw DiagnosticTraceError.sizeExceeded(actual: data.count, maximum: maximumBytes)
        }
        return data
    }

    public func decode(_ data: Data) throws -> DiagnosticTraceRecord {
        guard maximumRecords > 0, maximumBytes > 0 else { throw DiagnosticTraceError.invalidLimits }
        guard data.count <= maximumBytes else {
            throw DiagnosticTraceError.sizeExceeded(actual: data.count, maximum: maximumBytes)
        }
        do {
            let trace = try JSONDecoder().decode(DiagnosticTraceRecord.self, from: data)
            guard trace.schemaVersion == DiagnosticTraceContract.currentVersion else {
                throw DiagnosticTraceError.unsupportedVersion(trace.schemaVersion)
            }
            return trace.resanitized()
        } catch let error as DiagnosticTraceError {
            throw error
        } catch {
            throw DiagnosticTraceError.malformed
        }
    }
}

/// In-memory retention bound for callers that collect several traces.
public struct DiagnosticTraceBuffer: Codable, Sendable, Equatable {
    public let maximumRecords: Int
    public private(set) var records: [DiagnosticTraceRecord]

    public init(maximumRecords: Int = DiagnosticTraceContract.maxRecords, records: [DiagnosticTraceRecord] = []) {
        self.maximumRecords = max(1, maximumRecords)
        self.records = Array(records.suffix(max(1, maximumRecords)))
    }

    public mutating func append(_ trace: DiagnosticTraceRecord) {
        records.append(trace.sanitized())
        if records.count > maximumRecords {
            records.removeFirst(records.count - maximumRecords)
        }
    }
}

public enum DiagnosticTraceError: Error, Equatable, LocalizedError, Sendable {
    case invalidLimits
    case sizeExceeded(actual: Int, maximum: Int)
    case unsupportedVersion(Int)
    case malformed

    public var errorDescription: String? {
        switch self {
        case .invalidLimits: return "Diagnostic trace limits must be positive."
        case let .sizeExceeded(actual, maximum): return "Diagnostic trace is \(actual) bytes; the maximum is \(maximum) bytes."
        case let .unsupportedVersion(version): return "Diagnostic trace schema version \(version) is not supported."
        case .malformed: return "The diagnostic trace is malformed."
        }
    }
}

private enum DiagnosticTraceRedactor {
    private static let sensitiveKeys = [
        "password", "secret", "token", "credential", "authorization", "cookie", "private_key",
        "api_key", "apikey", "access_key", "text", "input", "value", "screenshot", "image", "path"
    ]

    static func redact(_ value: String) -> String {
        SymCockpitHistory.SecretRedactor.redact(
            String(value.prefix(DiagnosticTraceContract.maxStringLength))
        )
    }

    static func redact(_ fields: [String: String]) -> [String: String] {
        Dictionary(uniqueKeysWithValues: fields.map { key, value in
            let normalized = key.replacingOccurrences(of: "-", with: "_").lowercased()
            let isSensitive = sensitiveKeys.contains { normalized == $0 || normalized.contains($0) }
            return (key, isSensitive ? SymCockpitHistory.SecretRedactor.placeholder : redact(value))
        })
    }
}
