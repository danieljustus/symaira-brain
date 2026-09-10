import Foundation

/// The bounded set of delivery routes an action may use.
public enum ActionRoute: String, Codable, Sendable, CaseIterable {
    /// Invoke an action exposed by the target's Accessibility element.
    case semantic
    /// Post an input event at a coordinate identified by a fresh observation.
    case coordinate
    /// Use a native application or Accessibility API (for example focus/menu).
    case native
    /// Use the system-wide event path, which may change the user's focus.
    case foreground
}

/// The outcome of one route decision. A skipped route is diagnostic only; it
/// must never be retried without a new observation and an explicit decision.
public enum ActionRouteAttemptStatus: String, Codable, Sendable {
    case selected
    case skipped
    case succeeded
    case failed
    case refused
}

/// Secret-free evidence for one route considered by an action.
public struct ActionRouteAttempt: Codable, Sendable, Equatable {
    public let route: ActionRoute
    public let status: ActionRouteAttemptStatus
    public let reason: String?
    public let requiresFreshObservation: Bool

    public init(
        route: ActionRoute,
        status: ActionRouteAttemptStatus,
        reason: String? = nil,
        requiresFreshObservation: Bool = false
    ) {
        self.route = route
        self.status = status
        self.reason = reason
        self.requiresFreshObservation = requiresFreshObservation
    }
}

/// Bounded route selection and escalation evidence returned with an action.
public struct ActionRouteDiagnostics: Codable, Sendable, Equatable {
    public let route: ActionRoute?
    public let attempts: [ActionRouteAttempt]
    public let nextRoute: ActionRoute?
    public let requiresFreshObservation: Bool

    public init(
        route: ActionRoute?,
        attempts: [ActionRouteAttempt] = [],
        nextRoute: ActionRoute? = nil,
        requiresFreshObservation: Bool = false
    ) {
        self.route = route
        self.attempts = attempts
        self.nextRoute = nextRoute
        self.requiresFreshObservation = requiresFreshObservation
    }
}

/// Compatibility aliases for callers that use the shorter contract names.
public typealias RouteDiagnostics = ActionRouteDiagnostics
public typealias RouteAttempt = ActionRouteAttempt
public typealias RouteAttemptStatus = ActionRouteAttemptStatus

/// Deterministic, fail-closed route ordering. Foreground is always last.
public enum ActionRouteSelector {
    public static let safeOrdering: [ActionRoute] = [.semantic, .coordinate, .native, .foreground]

    /// Deduplicates routes and orders them independently of caller/container
    /// ordering, so a Set or unordered JSON object cannot change behavior.
    public static func ordered(_ routes: some Sequence<ActionRoute>) -> [ActionRoute] {
        let available = Set(routes)
        return safeOrdering.filter { available.contains($0) }
    }

    /// Selects the first available route in the safe ordering.
    public static func select(_ routes: some Sequence<ActionRoute>) -> ActionRoute? {
        ordered(routes).first
    }
}

/// Short compatibility name for the public selector contract.
public typealias RouteSelector = ActionRouteSelector
