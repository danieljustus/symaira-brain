import Foundation

/// Semantic operations understood by a terminal application adapter.
public enum TerminalSemanticAction: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case openShell = "open_shell"
    case runCommand = "run_command"
    case createSession = "create_session"
    case closeSession = "close_session"
    case selectSession = "select_session"
    case createTab = "create_tab"
    case closeTab = "close_tab"
    case selectTab = "select_tab"

    /// Compatibility spellings for clients that call creation "new".
    public static let newSession = Self.createSession
    public static let newTab = Self.createTab
}

/// Capability areas exposed by a terminal descriptor.
public enum TerminalCapability: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case shell
    case sessions
    case tabs
}

/// Versioned, Codable metadata for one terminal adapter.
public struct TerminalCapabilityDescriptor: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let capabilities: Set<TerminalCapability>
    public let semanticActions: Set<TerminalSemanticAction>
    public let supportsAXFallback: Bool

    public init(
        version: Int = TerminalCapabilityDescriptor.currentVersion,
        capabilities: Set<TerminalCapability> = [],
        semanticActions: Set<TerminalSemanticAction> = [],
        supportsAXFallback: Bool = true
    ) {
        self.version = version
        self.capabilities = capabilities
        self.semanticActions = semanticActions
        self.supportsAXFallback = supportsAXFallback
    }

    public func supports(_ action: TerminalSemanticAction) -> Bool {
        semanticActions.contains(action)
    }

    /// Alias that makes the metadata read naturally at call sites.
    public var actions: Set<TerminalSemanticAction> { semanticActions }

    /// Conservative metadata for Apple's Terminal.app. It describes routes; it
    /// does not perform automation by itself.
    public static let appleTerminal = TerminalCapabilityDescriptor(
        capabilities: [.shell, .sessions, .tabs],
        semanticActions: Set(TerminalSemanticAction.allCases),
        supportsAXFallback: true
    )

    private enum CodingKeys: String, CodingKey {
        case version, capabilities, semanticActions, supportsAXFallback
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let version = try container.decode(Int.self, forKey: .version)
        guard version == Self.currentVersion else {
            throw TerminalDescriptorError.unsupportedVersion(version)
        }
        self.version = version
        self.capabilities = try container.decode(Set<TerminalCapability>.self, forKey: .capabilities)
        self.semanticActions = try container.decode(Set<TerminalSemanticAction>.self, forKey: .semanticActions)
        self.supportsAXFallback = try container.decodeIfPresent(Bool.self, forKey: .supportsAXFallback) ?? true
    }
}

/// Identifies a terminal application and the routes it advertises.
public struct TerminalDescriptor: Codable, Equatable, Sendable {
    public let bundleIdentifier: String
    public let applicationName: String
    public let capabilityMetadata: TerminalCapabilityDescriptor

    public init(
        bundleIdentifier: String,
        applicationName: String,
        capabilityMetadata: TerminalCapabilityDescriptor
    ) {
        self.bundleIdentifier = bundleIdentifier
        self.applicationName = applicationName
        self.capabilityMetadata = capabilityMetadata
    }

    public var capabilities: TerminalCapabilityDescriptor { capabilityMetadata }

    public static let appleTerminal = TerminalDescriptor(
        bundleIdentifier: "com.apple.Terminal",
        applicationName: "Terminal",
        capabilityMetadata: .appleTerminal
    )
}

/// A request passed to either a semantic adapter or its AX fallback.
public struct TerminalActionRequest: Codable, Equatable, Sendable {
    public let action: TerminalSemanticAction
    public let identifier: String?
    public let snapshotID: String?
    public let elementID: String?
    public let axAction: String

    public init(
        action: TerminalSemanticAction,
        identifier: String? = nil,
        snapshotID: String? = nil,
        elementID: String? = nil,
        axAction: String = "AXPress"
    ) {
        self.action = action
        self.identifier = identifier
        self.snapshotID = snapshotID
        self.elementID = elementID
        self.axAction = axAction
    }

    public var hasAXReference: Bool {
        snapshotID?.isEmpty == false && elementID?.isEmpty == false
    }
}

public enum TerminalActionRoute: String, Codable, Equatable, Sendable {
    case semantic
    case ax
}

public struct TerminalActionResult: Codable, Equatable, Sendable {
    public let action: TerminalSemanticAction
    public let route: TerminalActionRoute
    public let message: String

    public init(action: TerminalSemanticAction, route: TerminalActionRoute, message: String) {
        self.action = action
        self.route = route
        self.message = message
    }
}

/// A semantic terminal adapter. Implementations must not silently perform an
/// AX action; callers use `TerminalActionRouter` when fallback is desired.
public protocol TerminalSemanticAdapterProtocol: Sendable {
    var descriptor: TerminalDescriptor { get }
    func perform(_ request: TerminalActionRequest) throws -> TerminalActionResult
}

/// Fallback boundary for the existing Accessibility service.
public protocol TerminalAXFallbackProtocol: Sendable {
    func performAX(_ request: TerminalActionRequest) throws -> TerminalActionResult
}

/// Selects the semantic route first and falls back to an explicit AX element.
/// Fallback is deliberately limited to capability/unavailability errors; real
/// operation failures are propagated rather than retried through another route.
public struct TerminalActionRouter: Sendable {
    private let semantic: any TerminalSemanticAdapterProtocol
    private let axFallback: any TerminalAXFallbackProtocol

    public init(
        semantic: any TerminalSemanticAdapterProtocol,
        axFallback: any TerminalAXFallbackProtocol
    ) {
        self.semantic = semantic
        self.axFallback = axFallback
    }

    public func perform(_ request: TerminalActionRequest) throws -> TerminalActionResult {
        guard semantic.descriptor.capabilityMetadata.supports(request.action) else {
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

    private func performAXFallback(_ request: TerminalActionRequest) throws -> TerminalActionResult {
        guard semantic.descriptor.capabilityMetadata.supportsAXFallback else {
            throw AutomationError.unsupported("No supported route is available for terminal action '\(request.action.rawValue)'.")
        }
        guard request.hasAXReference else {
            throw AutomationError.invalidArgument(
                "AX fallback for terminal action '\(request.action.rawValue)' requires snapshot_id and element_id."
            )
        }
        return try axFallback.performAX(request)
    }
}

/// Errors raised while decoding terminal capability metadata.
public enum TerminalDescriptorError: LocalizedError, Equatable {
    case unsupportedVersion(Int)

    public var errorDescription: String? {
        switch self {
        case .unsupportedVersion(let version):
            return "Unsupported terminal descriptor version \(version)."
        }
    }
}

/// Concrete bridge from the adapter protocol to the existing AX cache.
public struct AccessibilityTerminalFallback: TerminalAXFallbackProtocol, @unchecked Sendable {
    private let accessibility: any AccessibilityServiceProtocol

    public init(accessibility: any AccessibilityServiceProtocol) {
        self.accessibility = accessibility
    }

    public func performAX(_ request: TerminalActionRequest) throws -> TerminalActionResult {
        guard let snapshotID = request.snapshotID, let elementID = request.elementID else {
            throw AutomationError.invalidArgument("AX fallback requires snapshot_id and element_id.")
        }
        try accessibility.performElementAction(
            snapshotID: snapshotID,
            elementID: elementID,
            action: request.axAction
        )
        return TerminalActionResult(
            action: request.action,
            route: .ax,
            message: "Terminal action routed through Accessibility (\(request.axAction))."
        )
    }
}

public typealias TerminalAdapterProtocol = TerminalSemanticAdapterProtocol
public typealias TerminalAdapter = TerminalSemanticAdapterProtocol
public typealias AXTerminalFallback = AccessibilityTerminalFallback
