import Foundation

/// The macOS system UI surfaces that can be addressed by a semantic descriptor.
///
/// This contract is descriptive only. It does not discover or drive a real UI
/// element, which keeps callers responsible for obtaining a fresh observation.
public enum SystemUISurface: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case dock
    case menuBar = "menu_bar"
    case statusBar = "status_bar"
}

/// The only routes represented by the system-UI contract.
///
/// Coordinate and foreground delivery are intentionally absent. A missing or
/// unavailable semantic route must not silently turn into an unsafe click.
public enum SystemUIActionRoute: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case semantic
    case accessibilityFallback = "accessibility_fallback"
    case refused
}

/// The result of a pure, side-effect-free route decision.
public struct SystemUIRouteDecision: Codable, Equatable, Sendable {
    public let route: SystemUIActionRoute
    public let reason: String
    public let requiresFreshObservation: Bool

    public init(
        route: SystemUIActionRoute,
        reason: String,
        requiresFreshObservation: Bool = false
    ) {
        self.route = route
        self.reason = reason
        self.requiresFreshObservation = requiresFreshObservation
    }

    public var isExecutable: Bool {
        route != .refused
    }
}

/// Routing policy attached to a Dock, menu-bar, or status-bar descriptor.
///
/// The default is semantic-only and fail-closed. An Accessibility fallback is
/// opt-in and can only be selected when the caller has both an available AX
/// route and a fresh observation (for example, a current snapshot/element
/// reference). This type contains no coordinate or foreground fallback.
public struct SystemUIRoutingMetadata: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let preferredRoute: SystemUIActionRoute
    public let fallbackRoute: SystemUIActionRoute?
    public let fallbackRequiresFreshObservation: Bool
    public let allowsFallback: Bool

    public init(
        version: Int = SystemUIRoutingMetadata.currentVersion,
        preferredRoute: SystemUIActionRoute = .semantic,
        fallbackRoute: SystemUIActionRoute? = nil,
        fallbackRequiresFreshObservation: Bool = true,
        allowsFallback: Bool = false
    ) {
        self.version = version
        self.preferredRoute = preferredRoute
        self.fallbackRoute = fallbackRoute
        self.fallbackRequiresFreshObservation = fallbackRequiresFreshObservation
        self.allowsFallback = allowsFallback
    }

    /// The conservative policy used by every built-in descriptor.
    public static let semanticOnly = SystemUIRoutingMetadata()

    /// Opts into the existing AX path while preserving the fresh-observation
    /// boundary. This is not a coordinate or frontmost-app fallback.
    public static let semanticThenObservedAccessibility = SystemUIRoutingMetadata(
        fallbackRoute: .accessibilityFallback,
        fallbackRequiresFreshObservation: true,
        allowsFallback: true
    )

    /// Chooses a route without performing any UI operation.
    public func decision(
        semanticAvailable: Bool,
        accessibilityFallbackAvailable: Bool = false,
        hasFreshObservation: Bool = false
    ) -> SystemUIRouteDecision {
        if preferredRoute == .semantic, semanticAvailable {
            return SystemUIRouteDecision(
                route: .semantic,
                reason: "The advertised semantic route is available."
            )
        }

        guard allowsFallback,
              fallbackRoute == .accessibilityFallback,
              accessibilityFallbackAvailable else {
            return SystemUIRouteDecision(
                route: .refused,
                reason: "No safe semantic route is available; the action was refused."
            )
        }

        if fallbackRequiresFreshObservation && !hasFreshObservation {
            return SystemUIRouteDecision(
                route: .refused,
                reason: "Accessibility fallback requires a fresh UI observation.",
                requiresFreshObservation: true
            )
        }

        return SystemUIRouteDecision(
            route: .accessibilityFallback,
            reason: "Semantic delivery is unavailable; using the explicitly enabled Accessibility fallback."
        )
    }

    /// Compatibility spelling for callers that call the policy a selector.
    public func select(
        semanticAvailable: Bool,
        accessibilityFallbackAvailable: Bool = false,
        hasFreshObservation: Bool = false
    ) -> SystemUIRouteDecision {
        decision(
            semanticAvailable: semanticAvailable,
            accessibilityFallbackAvailable: accessibilityFallbackAvailable,
            hasFreshObservation: hasFreshObservation
        )
    }
}

/// Common metadata exposed by each typed system-UI descriptor.
public protocol SystemUISemanticDescriptorProtocol: Codable, Equatable, Sendable {
    associatedtype Action: RawRepresentable & Codable & Hashable & Sendable where Action.RawValue == String

    var version: Int { get }
    var surface: SystemUISurface { get }
    var bundleIdentifier: String? { get }
    var applicationName: String? { get }
    var itemIdentifier: String? { get }
    var itemTitle: String? { get }
    var supportedActions: Set<Action> { get }
    var routing: SystemUIRoutingMetadata { get }

    func supports(_ action: Action) -> Bool
}

public extension SystemUISemanticDescriptorProtocol {
    /// Resolves routing for one advertised action and refuses unsupported
    /// actions before considering any fallback route.
    func routeDecision(
        for action: Action,
        semanticAvailable: Bool,
        accessibilityFallbackAvailable: Bool = false,
        hasFreshObservation: Bool = false
    ) -> SystemUIRouteDecision {
        guard supports(action) else {
            return SystemUIRouteDecision(
                route: .refused,
                reason: "The requested action is not advertised by this descriptor."
            )
        }
        return routing.decision(
            semanticAvailable: semanticAvailable,
            accessibilityFallbackAvailable: accessibilityFallbackAvailable,
            hasFreshObservation: hasFreshObservation
        )
    }
}

/// Semantic actions that can be addressed in the Dock.
public enum DockSemanticAction: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case activate
    case openContextMenu = "open_context_menu"
}

/// A stable description of one Dock item.
public struct DockSemanticDescriptor: SystemUISemanticDescriptorProtocol {
    public static let currentVersion = 1

    public let version: Int
    public var surface: SystemUISurface { .dock }
    public let bundleIdentifier: String?
    public let applicationName: String?
    public let itemIdentifier: String?
    public let itemTitle: String?
    public let supportedActions: Set<DockSemanticAction>
    public let routing: SystemUIRoutingMetadata

    public init(
        version: Int = DockSemanticDescriptor.currentVersion,
        bundleIdentifier: String? = nil,
        applicationName: String? = nil,
        itemIdentifier: String? = nil,
        itemTitle: String? = nil,
        supportedActions: Set<DockSemanticAction> = Set(DockSemanticAction.allCases),
        routing: SystemUIRoutingMetadata = .semanticOnly
    ) {
        self.version = version
        self.bundleIdentifier = bundleIdentifier
        self.applicationName = applicationName
        self.itemIdentifier = itemIdentifier
        self.itemTitle = itemTitle
        self.supportedActions = supportedActions
        self.routing = routing
    }

    public func supports(_ action: DockSemanticAction) -> Bool {
        supportedActions.contains(action)
    }

    public var capabilities: Set<String> {
        Set(supportedActions.map(\.rawValue))
    }
}

/// Semantic actions that can be addressed in an application's menu bar.
public enum MenuBarSemanticAction: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case openMenu = "open_menu"
    case selectItem = "select_item"
}

/// A stable description of one menu-bar path/item.
public struct MenuBarSemanticDescriptor: SystemUISemanticDescriptorProtocol {
    public static let currentVersion = 1

    public let version: Int
    public var surface: SystemUISurface { .menuBar }
    public let bundleIdentifier: String?
    public let applicationName: String?
    public let itemIdentifier: String?
    public let itemTitle: String?
    public let menuPath: [String]
    public let supportedActions: Set<MenuBarSemanticAction>
    public let routing: SystemUIRoutingMetadata

    public init(
        version: Int = MenuBarSemanticDescriptor.currentVersion,
        bundleIdentifier: String? = nil,
        applicationName: String? = nil,
        menuPath: [String] = [],
        itemIdentifier: String? = nil,
        itemTitle: String? = nil,
        supportedActions: Set<MenuBarSemanticAction> = Set(MenuBarSemanticAction.allCases),
        routing: SystemUIRoutingMetadata = .semanticOnly
    ) {
        self.version = version
        self.bundleIdentifier = bundleIdentifier
        self.applicationName = applicationName
        self.menuPath = menuPath
        self.itemIdentifier = itemIdentifier
        self.itemTitle = itemTitle
        self.supportedActions = supportedActions
        self.routing = routing
    }

    public func supports(_ action: MenuBarSemanticAction) -> Bool {
        supportedActions.contains(action)
    }

    public var capabilities: Set<String> {
        Set(supportedActions.map(\.rawValue))
    }
}

/// Semantic actions that can be addressed in a menu-bar status item.
public enum StatusBarSemanticAction: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case activate
    case openMenu = "open_menu"
}

/// A stable description of one status-bar item.
public struct StatusBarSemanticDescriptor: SystemUISemanticDescriptorProtocol {
    public static let currentVersion = 1

    public let version: Int
    public var surface: SystemUISurface { .statusBar }
    public let bundleIdentifier: String?
    public let applicationName: String?
    public let itemIdentifier: String?
    public let itemTitle: String?
    public let supportedActions: Set<StatusBarSemanticAction>
    public let routing: SystemUIRoutingMetadata

    public init(
        version: Int = StatusBarSemanticDescriptor.currentVersion,
        bundleIdentifier: String? = nil,
        applicationName: String? = nil,
        itemIdentifier: String? = nil,
        itemTitle: String? = nil,
        supportedActions: Set<StatusBarSemanticAction> = Set(StatusBarSemanticAction.allCases),
        routing: SystemUIRoutingMetadata = .semanticOnly
    ) {
        self.version = version
        self.bundleIdentifier = bundleIdentifier
        self.applicationName = applicationName
        self.itemIdentifier = itemIdentifier
        self.itemTitle = itemTitle
        self.supportedActions = supportedActions
        self.routing = routing
    }

    public func supports(_ action: StatusBarSemanticAction) -> Bool {
        supportedActions.contains(action)
    }

    public var capabilities: Set<String> {
        Set(supportedActions.map(\.rawValue))
    }
}

/// Compatibility spellings for clients that use "menu bar" as one word.
public typealias MenubarSemanticDescriptor = MenuBarSemanticDescriptor
public typealias MenubarSemanticAction = MenuBarSemanticAction
public typealias StatusItemSemanticDescriptor = StatusBarSemanticDescriptor
public typealias StatusItemSemanticAction = StatusBarSemanticAction
public typealias DockCapabilityMetadata = DockSemanticDescriptor
public typealias MenuBarCapabilityMetadata = MenuBarSemanticDescriptor
public typealias StatusBarCapabilityMetadata = StatusBarSemanticDescriptor
public typealias SystemUIRouteMetadata = SystemUIRoutingMetadata
public typealias SystemUIRoute = SystemUIActionRoute
