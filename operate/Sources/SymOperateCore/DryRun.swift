import Foundation

public enum DryRunMode: String, Codable, Sendable {
    case plan
    case execute
}

public struct DryRunAction: Codable, Sendable, Equatable {
    public let name: String
    public let permission: PermissionFlags
    public let target: ActionTarget?
    public let conditions: ActionConditions?

    public init(name: String, permission: PermissionFlags, target: ActionTarget? = nil, conditions: ActionConditions? = nil) {
        self.name = name
        self.permission = permission
        self.target = target
        self.conditions = conditions
    }
}

public struct DryRunEntry: Codable, Sendable, Equatable {
    public let action: DryRunAction
    public let allowed: Bool
    public let reason: String?

    public init(action: DryRunAction, allowed: Bool, reason: String? = nil) {
        self.action = action
        self.allowed = allowed
        self.reason = reason
    }
}

public struct DryRunPlan: Codable, Sendable, Equatable {
    public let entries: [DryRunEntry]
    public let executable: Bool

    public init(entries: [DryRunEntry]) {
        self.entries = entries
        self.executable = entries.allSatisfy(\.allowed)
    }
}

/// Plans actions without invoking input, application, or window side effects.
public struct DryRunPlanner: Sendable {
    private let policy: ActionPolicy

    public init(policy: ActionPolicy) {
        self.policy = policy
    }

    public func plan(_ actions: [DryRunAction]) -> DryRunPlan {
        let entries = actions.map { action in
            if let violation = policy.firstViolation(requiredPermission: action.permission) {
                return DryRunEntry(action: action, allowed: false, reason: String(describing: violation))
            }
            return DryRunEntry(action: action, allowed: true)
        }
        return DryRunPlan(entries: entries)
    }
}
