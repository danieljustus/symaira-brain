import Foundation

/// The three native UI surfaces that can be described without relying on
/// coordinates. Their AX roles are stable, but matching remains tolerant of
/// the other metadata being absent.
public enum NativeControlKind: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case toolbar
    case sheet
    case control

    public var accessibilityRole: String? {
        switch self {
        case .toolbar: return "AXToolbar"
        case .sheet: return "AXSheet"
        case .control: return nil
        }
    }
}

/// A small, transport-safe description of one native toolbar, sheet, or
/// interactive control. It is intentionally expressed in terms of AX metadata
/// so callers can match a fresh UI snapshot without touching live AX objects.
public struct NativeControlDescriptor: Codable, Equatable, Sendable {
    public let kind: NativeControlKind
    public let role: String?
    public let title: String?
    public let label: String?
    public let value: String?
    public let identifier: String?
    public let enabled: Bool?
    public let actions: Set<String>?
    public let requiredControls: [NativeControlDescriptor]
    public let supportsAXFallback: Bool

    public init(
        kind: NativeControlKind,
        role: String? = nil,
        title: String? = nil,
        label: String? = nil,
        value: String? = nil,
        identifier: String? = nil,
        enabled: Bool? = nil,
        actions: Set<String>? = nil,
        requiredControls: [NativeControlDescriptor] = [],
        supportsAXFallback: Bool = true
    ) {
        self.kind = kind
        self.role = role
        self.title = title
        self.label = label
        self.value = value
        self.identifier = identifier
        self.enabled = enabled
        self.actions = actions
        self.requiredControls = requiredControls
        self.supportsAXFallback = supportsAXFallback
    }

    public static func toolbar(
        title: String? = nil,
        label: String? = nil,
        identifier: String? = nil,
        requiredControls: [NativeControlDescriptor] = [],
        supportsAXFallback: Bool = true
    ) -> Self {
        Self(kind: .toolbar, title: title, label: label, identifier: identifier,
             requiredControls: requiredControls, supportsAXFallback: supportsAXFallback)
    }

    public static func sheet(
        title: String? = nil,
        label: String? = nil,
        identifier: String? = nil,
        requiredControls: [NativeControlDescriptor] = [],
        supportsAXFallback: Bool = true
    ) -> Self {
        Self(kind: .sheet, title: title, label: label, identifier: identifier,
             requiredControls: requiredControls, supportsAXFallback: supportsAXFallback)
    }

    public static func control(
        role: String,
        title: String? = nil,
        label: String? = nil,
        value: String? = nil,
        identifier: String? = nil,
        enabled: Bool? = nil,
        actions: Set<String>? = ["AXPress"],
        supportsAXFallback: Bool = true
    ) -> Self {
        Self(kind: .control, role: role, title: title, label: label, value: value,
             identifier: identifier, enabled: enabled, actions: actions,
             supportsAXFallback: supportsAXFallback)
    }

    /// The generic predicate used by the existing Accessibility query layer.
    /// Identifiers are represented by `UINode.label`, which is where the AX
    /// snapshot builder stores AXDescription/AXIdentifier.
    public var accessibilityPredicate: UIElementPredicate {
        UIElementPredicate(
            role: role ?? kind.accessibilityRole,
            title: title,
            label: label ?? identifier,
            value: value,
            actions: actions.map(Array.init),
            enabled: enabled
        )
    }

    /// Matches only snapshot data. No live application, AX permission, or
    /// coordinate lookup is involved, making this helper deterministic in tests.
    public func matches(node: UINode) -> Bool {
        guard kind == .control || node.role == kind.accessibilityRole else { return false }
        guard accessibilityPredicate.matches(node: node) else { return false }
        return requiredControls.allSatisfy { required in
            UIQueryService().findFirstNode(in: node.children, predicate: required.accessibilityPredicate)
                .map(required.matches(node:)) == true
        }
    }

    public func matches(_ node: UINode) -> Bool { matches(node: node) }
}

/// Names the route used for one native-control action.
public enum NativeControlActionRoute: String, Codable, Equatable, Sendable {
    case semantic
    case axFallback = "ax_fallback"
}

/// A request shared by native implementations and the conservative AX route.
public struct NativeControlActionRequest: Codable, Equatable, Sendable {
    public let descriptor: NativeControlDescriptor
    public let snapshotID: String?
    public let elementID: String?
    public let axAction: String

    public init(
        descriptor: NativeControlDescriptor,
        snapshotID: String? = nil,
        elementID: String? = nil,
        axAction: String = "AXPress"
    ) {
        self.descriptor = descriptor
        self.snapshotID = snapshotID
        self.elementID = elementID
        self.axAction = axAction
    }

    public var hasAXReference: Bool {
        snapshotID?.isEmpty == false && elementID?.isEmpty == false
    }
}

public struct NativeControlActionResult: Codable, Equatable, Sendable {
    public let kind: NativeControlKind
    public let route: NativeControlActionRoute
    public let message: String

    public init(kind: NativeControlKind, route: NativeControlActionRoute, message: String) {
        self.kind = kind
        self.route = route
        self.message = message
    }
}

/// Native implementations may be supplied by an app-specific adapter. The
/// generic layer never guesses an application-specific native API.
public protocol NativeControlSemanticAdapterProtocol: Sendable {
    func perform(_ request: NativeControlActionRequest) throws -> NativeControlActionResult
}

/// Boundary for the existing AX cache. Implementations must receive an
/// observed snapshot and element identity; coordinate or fresh-tree guessing
/// is intentionally not part of this fallback contract.
public protocol NativeControlAXFallbackProtocol: Sendable {
    func performAX(_ request: NativeControlActionRequest) throws -> NativeControlActionResult
}

/// Prefer the native route. Fall back only for an explicitly unsupported or
/// unavailable native route, and only when the request carries a fresh AX
/// reference. Semantic operation failures are never retried through AX.
public struct NativeControlActionRouter: Sendable {
    private let semantic: (any NativeControlSemanticAdapterProtocol)?
    private let axFallback: (any NativeControlAXFallbackProtocol)?

    public init(
        semantic: (any NativeControlSemanticAdapterProtocol)?,
        axFallback: (any NativeControlAXFallbackProtocol)?
    ) {
        self.semantic = semantic
        self.axFallback = axFallback
    }

    public func perform(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        guard request.descriptor.supportsAXFallback else {
            return try performSemanticOnly(request)
        }
        guard let semantic else {
            return try performAXFallback(request)
        }

        do {
            return try semantic.perform(request)
        } catch let error as AutomationError {
            switch error {
            case .unsupported, .unavailable:
                return try performAXFallback(request)
            default:
                throw error
            }
        }
    }

    private func performSemanticOnly(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        guard let semantic else {
            throw AutomationError.unsupported("No supported route is available for native \(request.descriptor.kind.rawValue) control.")
        }
        return try semantic.perform(request)
    }

    private func performAXFallback(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        guard let axFallback else {
            throw AutomationError.unsupported("No Accessibility fallback is available for native \(request.descriptor.kind.rawValue) control.")
        }
        guard request.hasAXReference else {
            throw AutomationError.invalidArgument(
                "AX fallback for native \(request.descriptor.kind.rawValue) control requires snapshot_id and element_id."
            )
        }
        return try axFallback.performAX(request)
    }
}

/// Concrete bridge from the generic router to the existing Accessibility
/// service and its ephemeral snapshot cache.
public struct AccessibilityNativeControlFallback: NativeControlAXFallbackProtocol, @unchecked Sendable {
    private let accessibility: any AccessibilityServiceProtocol

    public init(accessibility: any AccessibilityServiceProtocol) {
        self.accessibility = accessibility
    }

    public func performAX(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        guard let snapshotID = request.snapshotID, let elementID = request.elementID else {
            throw AutomationError.invalidArgument("AX fallback requires snapshot_id and element_id.")
        }
        try accessibility.performElementAction(snapshotID: snapshotID, elementID: elementID, action: request.axAction)
        return NativeControlActionResult(
            kind: request.descriptor.kind,
            route: .axFallback,
            message: "Native \(request.descriptor.kind.rawValue) action routed through Accessibility (\(request.axAction))."
        )
    }
}

/// Convenience spellings for callers that model each native surface separately.
public typealias NativeToolbarDescriptor = NativeControlDescriptor
public typealias NativeSheetDescriptor = NativeControlDescriptor
public typealias NativeControlSemanticDescriptor = NativeControlDescriptor
public typealias ToolbarSemanticDescriptor = NativeControlDescriptor
public typealias SheetSemanticDescriptor = NativeControlDescriptor
public typealias ControlSemanticDescriptor = NativeControlDescriptor
public typealias ToolbarDescriptor = NativeControlDescriptor
public typealias SheetDescriptor = NativeControlDescriptor
public typealias ControlDescriptor = NativeControlDescriptor

/// Pure matching helpers for callers that do not need to instantiate a query
/// service. These operate on immutable snapshot nodes only.
public enum NativeControlMatcher {
    public static func matches(_ node: UINode, descriptor: NativeControlDescriptor) -> Bool {
        descriptor.matches(node: node)
    }

    public static func findFirst(
        in nodes: [UINode],
        descriptor: NativeControlDescriptor
    ) -> UINode? {
        for node in nodes {
            if descriptor.matches(node: node) { return node }
            if let match = findFirst(in: node.children, descriptor: descriptor) { return match }
        }
        return nil
    }
}

public extension UIQueryService {
    func findNodes(in nodes: [UINode], descriptor: NativeControlDescriptor) -> [UINode] {
        findNodes(in: nodes, predicate: descriptor.accessibilityPredicate)
            .filter(descriptor.matches(node:))
    }

    func findFirstNode(in nodes: [UINode], descriptor: NativeControlDescriptor) -> UINode? {
        findNodes(in: nodes, descriptor: descriptor).first
    }
}
