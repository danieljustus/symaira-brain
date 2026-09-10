import Foundation

/// The result of evaluating a bounded action predicate against one UI observation.
public enum PredicateEvaluationStatus: String, Codable, Sendable {
    case satisfied
    case failed
    case unverifiable
}

public struct PredicateEvaluation: Codable, Sendable {
    public let status: PredicateEvaluationStatus
    public let predicate: UIElementPredicate?
    public let observation: UIQueryResult?
    public let reason: String?

    public init(
        status: PredicateEvaluationStatus,
        predicate: UIElementPredicate?,
        observation: UIQueryResult?,
        reason: String? = nil
    ) {
        self.status = status
        self.predicate = predicate
        self.observation = observation
        self.reason = reason
    }
}

/// Optional verification attached to a state-changing action. The fields are
/// optional so existing callers keep the original fire-and-forget contract.
public struct ActionConditions: Codable, Sendable, Equatable {
    public let precondition: UIElementPredicate?
    public let postcondition: UIElementPredicate?

    public init(precondition: UIElementPredicate? = nil, postcondition: UIElementPredicate? = nil) {
        self.precondition = precondition
        self.postcondition = postcondition
    }
}

public struct ActionConditionsResult: Codable, Sendable {
    public let precondition: PredicateEvaluation?
    public let postcondition: PredicateEvaluation

    public init(precondition: PredicateEvaluation?, postcondition: PredicateEvaluation) {
        self.precondition = precondition
        self.postcondition = postcondition
    }
}

extension UIElementPredicate {
    var hasNodeCriteria: Bool {
        role != nil || title != nil || label != nil || value != nil
            || subrole != nil || actions != nil || text != nil || enabled != nil
    }
}
