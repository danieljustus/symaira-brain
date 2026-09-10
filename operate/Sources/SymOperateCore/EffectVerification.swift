import Foundation

/// Version of the additive action effect contract exposed to MCP clients.
public enum EffectContract {
    public static let currentVersion = 1
}

/// Describes how far a state-changing action progressed.
public enum EffectState: String, Codable, Sendable {
    case submitted
    case confirmed
    case unverifiable
    case suspectedNoop = "suspected_noop"
    case refused
}

/// Compatibility alias for callers that prefer the action-prefixed name.
public typealias ActionEffect = EffectState

/// Describes the evidence available after an action was submitted.
public enum VerificationStatus: String, Codable, Sendable {
    case confirmed
    case unverifiable
    case suspectedNoop = "suspected_noop"
    case notAttempted = "not_attempted"
}

/// Compatibility alias for callers that prefer the action-prefixed name.
public typealias ActionVerificationStatus = VerificationStatus

public struct ActionVerification: Codable, Sendable, Equatable {
    public let status: VerificationStatus
    public let strategy: String
    public let reason: String?
    public let snapshotID: String?
    public let checkedAt: String?

    public init(
        status: VerificationStatus,
        strategy: String,
        reason: String? = nil,
        snapshotID: String? = nil,
        checkedAt: String? = DateFormats.iso8601String(from: Date())
    ) {
        self.status = status
        self.strategy = strategy
        self.reason = reason
        self.snapshotID = snapshotID
        self.checkedAt = checkedAt
    }
}

/// Identifies the requested and observed target where the platform exposes it.
public struct ActionTarget: Codable, Sendable, Equatable {
    public let requestedPID: Int32?
    public let requestedBundleID: String?
    public let requestedAppName: String?
    public let requestedWindowID: Int?
    public let requestedWindowTitle: String?
    public let requestedDisplayID: UInt32?
    public let requestedSpaceID: Int?
    public let frontmostPID: Int32?
    public let frontmostWindowID: Int?

    public init(
        requestedPID: Int32? = nil,
        requestedBundleID: String? = nil,
        requestedAppName: String? = nil,
        requestedWindowID: Int? = nil,
        requestedWindowTitle: String? = nil,
        requestedDisplayID: UInt32? = nil,
        requestedSpaceID: Int? = nil,
        frontmostPID: Int32? = nil,
        frontmostWindowID: Int? = nil
    ) {
        self.requestedPID = requestedPID
        self.requestedBundleID = requestedBundleID
        self.requestedAppName = requestedAppName
        self.requestedWindowID = requestedWindowID
        self.requestedWindowTitle = requestedWindowTitle
        self.requestedDisplayID = requestedDisplayID
        self.requestedSpaceID = requestedSpaceID
        self.frontmostPID = frontmostPID
        self.frontmostWindowID = frontmostWindowID
    }
}

/// The versioned, additive outcome contract for automation actions.
public struct ActionResult: Codable, Sendable {
    // Legacy fields retained unchanged for existing MCP clients.
    public let ok: Bool
    public let message: String
    public let snapshot: Snapshot?

    public let contractVersion: Int
    public let effect: EffectState
    public let verification: ActionVerification
    public let executionPath: String?
    public let deliveryMode: DeliveryMode
    public let target: ActionTarget?
    /// Bounded route selection and escalation evidence for this action.
    public let routeDiagnostics: ActionRouteDiagnostics?
    /// Optional pre/postcondition evidence from the action-conditions contract.
    public let conditions: ActionConditionsResult?

    public init(
        ok: Bool,
        message: String,
        snapshot: Snapshot? = nil,
        contractVersion: Int = EffectContract.currentVersion,
        effect: EffectState = .submitted,
        verification: ActionVerification = ActionVerification(
            status: .unverifiable,
            strategy: "not_attempted",
            reason: "No post-action verification was requested."
        ),
        executionPath: String? = nil,
        deliveryMode: DeliveryMode = .automatic,
        target: ActionTarget? = nil,
        routeDiagnostics: ActionRouteDiagnostics? = nil,
        conditions: ActionConditionsResult? = nil
    ) {
        self.ok = ok
        self.message = message
        self.snapshot = snapshot
        self.contractVersion = contractVersion
        self.effect = effect
        self.verification = verification
        self.executionPath = executionPath
        self.deliveryMode = deliveryMode
        self.target = target
        self.routeDiagnostics = routeDiagnostics
        self.conditions = conditions
    }

    private enum CodingKeys: String, CodingKey {
        case ok, message, snapshot, contractVersion, effect, verification, executionPath, deliveryMode, target, routeDiagnostics, conditions
    }

    /// New clients can decode legacy three-field results; legacy results are
    /// conservatively classified as submitted and unverifiable.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        ok = try container.decode(Bool.self, forKey: .ok)
        message = try container.decode(String.self, forKey: .message)
        snapshot = try container.decodeIfPresent(Snapshot.self, forKey: .snapshot)
        contractVersion = try container.decodeIfPresent(Int.self, forKey: .contractVersion) ?? 0
        effect = try container.decodeIfPresent(EffectState.self, forKey: .effect) ?? (ok ? .submitted : .refused)
        verification = try container.decodeIfPresent(ActionVerification.self, forKey: .verification)
            ?? ActionVerification(
                status: .unverifiable,
                strategy: "legacy_result",
                reason: "The result predates the effect verification contract.",
                snapshotID: snapshot?.id
            )
        executionPath = try container.decodeIfPresent(String.self, forKey: .executionPath)
        deliveryMode = try container.decodeIfPresent(DeliveryMode.self, forKey: .deliveryMode) ?? .automatic
        target = try container.decodeIfPresent(ActionTarget.self, forKey: .target)
        routeDiagnostics = try container.decodeIfPresent(ActionRouteDiagnostics.self, forKey: .routeDiagnostics)
        conditions = try container.decodeIfPresent(ActionConditionsResult.self, forKey: .conditions)
    }
}
