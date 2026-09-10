import Foundation

/// Describes one bounded action that can compensate a completed workflow step.
///
/// A compensation is an explicit action supplied by the caller. The runner never
/// infers an undo action from the original action name or its parameters.
public struct WorkflowCompensation: Codable, Sendable, Equatable {
    public let action: String
    public let parameters: [String: String]
    public let conditions: ActionConditions

    public init(
        action: String,
        parameters: [String: String] = [:],
        conditions: ActionConditions = ActionConditions()
    ) {
        self.action = action
        self.parameters = parameters
        self.conditions = conditions
    }
}

/// Explicit recovery metadata for one workflow step.
///
/// `unavailable` is intentionally distinct from an omitted compensation: callers
/// can report that recovery is unsupported instead of guessing a reversal.
public enum WorkflowRecovery: Codable, Sendable, Equatable {
    case unavailable
    case compensation(WorkflowCompensation)

    public static func reversible(_ compensation: WorkflowCompensation) -> Self {
        .compensation(compensation)
    }

    public var isAvailable: Bool {
        if case .compensation = self { return true }
        return false
    }

    public var compensation: WorkflowCompensation? {
        if case .compensation(let compensation) = self { return compensation }
        return nil
    }

    private enum CodingKeys: String, CodingKey {
        case kind
        case compensation
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .unavailable:
            try container.encode("unavailable", forKey: .kind)
        case .compensation(let compensation):
            try container.encode("compensation", forKey: .kind)
            try container.encode(compensation, forKey: .compensation)
        }
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(String.self, forKey: .kind) {
        case "unavailable":
            self = .unavailable
        case "compensation":
            self = .compensation(try container.decode(WorkflowCompensation.self, forKey: .compensation))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .kind,
                in: container,
                debugDescription: "Unknown workflow recovery kind."
            )
        }
    }
}

/// Compatibility spelling for callers that refer to step recovery directly.
public typealias WorkflowStepRecovery = WorkflowRecovery

/// Outcome of one attempted recovery hook.
public enum WorkflowCompensationStatus: String, Codable, Sendable {
    case completed
    case unavailable
    case refused
    case failed
}

public struct WorkflowCompensationResult: Codable, Sendable {
    public let stepID: String
    public let action: String
    public let status: WorkflowCompensationStatus
    public let result: ActionResult?
    public let message: String

    public init(
        stepID: String,
        action: String,
        status: WorkflowCompensationStatus,
        result: ActionResult? = nil,
        message: String
    ) {
        self.stepID = stepID
        self.action = action
        self.status = status
        self.result = result
        self.message = message
    }
}
