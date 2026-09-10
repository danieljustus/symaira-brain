import AppKit
import Foundation

/// The small set of Finder actions that can be represented semantically.
public enum FinderSemanticAction: String, Codable, CaseIterable, Hashable, Sendable {
    case open
    case select
    case reveal
    case search
}

/// A target for a Finder semantic action.
///
/// Item actions use a filesystem path. Search uses the user's query verbatim;
/// no shell or AppleScript interpolation is performed by the descriptor layer.
public enum FinderSemanticTarget: Codable, Equatable, Sendable {
    case item(path: String)
    case search(query: String)

    public static func path(_ path: String) -> Self {
        .item(path: path)
    }

    public static func query(_ query: String) -> Self {
        .search(query: query)
    }
}

/// Describes which semantic Finder actions are safe to attempt.
public struct FinderCapabilityMetadata: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let appFamily: String
    public let bundleIdentifier: String
    public let supportedActions: Set<FinderSemanticAction>
    public let supportsAXFallback: Bool

    public init(
        version: Int = FinderCapabilityMetadata.currentVersion,
        appFamily: String = "finder",
        bundleIdentifier: String = "com.apple.finder",
        supportedActions: Set<FinderSemanticAction> = Set(FinderSemanticAction.allCases),
        supportsAXFallback: Bool = true
    ) {
        self.version = version
        self.appFamily = appFamily
        self.bundleIdentifier = bundleIdentifier
        self.supportedActions = supportedActions
        self.supportsAXFallback = supportsAXFallback
    }

    public static let finder = FinderCapabilityMetadata()

    public func supports(_ action: FinderSemanticAction) -> Bool {
        supportedActions.contains(action)
    }

    public var supportsOpen: Bool { supports(.open) }
    public var supportsSelect: Bool { supports(.select) }
    public var supportsReveal: Bool { supports(.reveal) }
    public var supportsSearch: Bool { supports(.search) }

    /// String names are useful when exposing metadata in a generic capability response.
    public var capabilities: Set<String> {
        Set(supportedActions.map(\.rawValue))
    }
}

/// A protocol shared by semantic descriptors so adapters can inspect an action
/// without depending on a concrete descriptor implementation.
public protocol FinderSemanticDescriptorProtocol: Sendable {
    var action: FinderSemanticAction { get }
    var target: FinderSemanticTarget { get }
    var capability: FinderCapabilityMetadata { get }
}

/// A validated, transport-friendly description of one Finder operation.
public struct FinderSemanticDescriptor: FinderSemanticDescriptorProtocol, Codable, Equatable, Sendable {
    public let action: FinderSemanticAction
    public let target: FinderSemanticTarget
    public let capability: FinderCapabilityMetadata

    public init(
        action: FinderSemanticAction,
        target: FinderSemanticTarget,
        capability: FinderCapabilityMetadata = .finder
    ) throws {
        try Self.validate(action: action, target: target)
        self.action = action
        self.target = target
        self.capability = capability
    }

    private static func validate(action: FinderSemanticAction, target: FinderSemanticTarget) throws {
        switch (action, target) {
        case (.search, .search(let query)):
            guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw FinderSemanticAdapterError.invalidTarget("Finder search query must not be empty.")
            }
        case (.search, .item):
            throw FinderSemanticAdapterError.invalidTarget("Finder search requires a search query target.")
        case (.open, .item(let path)), (.select, .item(let path)), (.reveal, .item(let path)):
            guard !path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw FinderSemanticAdapterError.invalidTarget("Finder item path must not be empty.")
            }
        case (.open, .search), (.select, .search), (.reveal, .search):
            throw FinderSemanticAdapterError.invalidTarget("Finder item action requires a filesystem path target.")
        }
    }
}

/// Compatibility spelling for callers that treat a descriptor as a protocol contract.
public typealias FinderActionDescriptor = FinderSemanticDescriptor

public enum FinderActionRoute: String, Codable, Equatable, Sendable {
    case semantic
    case axFallback
}

public struct FinderActionResult: Codable, Equatable, Sendable {
    public let action: FinderSemanticAction
    public let route: FinderActionRoute
    public let message: String

    public init(action: FinderSemanticAction, route: FinderActionRoute, message: String) {
        self.action = action
        self.route = route
        self.message = message
    }
}

/// Errors that control whether the router may safely try the AX fallback.
public enum FinderSemanticAdapterError: LocalizedError, Equatable, Sendable {
    case unsupportedAction(FinderSemanticAction)
    case unavailable(String)
    case invalidTarget(String)
    case operationFailed(String)

    /// Only capability absence and adapter unavailability are fallback-safe.
    public var permitsAXFallback: Bool {
        switch self {
        case .unsupportedAction, .unavailable:
            true
        case .invalidTarget, .operationFailed:
            false
        }
    }

    public var errorDescription: String? {
        switch self {
        case .unsupportedAction(let action):
            return "Finder semantic action '\(action.rawValue)' is unsupported."
        case .unavailable(let message), .invalidTarget(let message), .operationFailed(let message):
            return message
        }
    }
}

/// Builds and executes semantic Finder descriptors.
public protocol FinderSemanticAdapterProtocol: Sendable {
    var capabilityMetadata: FinderCapabilityMetadata { get }
    func descriptor(for action: FinderSemanticAction, target: FinderSemanticTarget) throws -> FinderSemanticDescriptor
    func perform(_ descriptor: FinderSemanticDescriptor) throws -> FinderActionResult
}

/// The AX fallback is deliberately injected: callers choose the observed AX
/// element/menu route instead of this layer guessing at Finder UI structure.
public protocol FinderAXFallbackProtocol: Sendable {
    func performAXFallback(for descriptor: FinderSemanticDescriptor) throws -> FinderActionResult
}

/// Routes supported Finder operations through the semantic adapter and falls
/// back to a caller-provided, already-observed AX route only when it is safe.
public struct FinderActionRouter: Sendable {
    public let adapter: any FinderSemanticAdapterProtocol
    public let axFallback: (any FinderAXFallbackProtocol)?

    public init(
        adapter: any FinderSemanticAdapterProtocol,
        axFallback: (any FinderAXFallbackProtocol)? = nil
    ) {
        self.adapter = adapter
        self.axFallback = axFallback
    }

    public func perform(
        _ action: FinderSemanticAction,
        target: FinderSemanticTarget
    ) throws -> FinderActionResult {
        let descriptor = try adapter.descriptor(for: action, target: target)
        guard adapter.capabilityMetadata.supports(action) else {
            return try fallbackOrThrow(descriptor, error: .unsupportedAction(action))
        }

        do {
            return try adapter.perform(descriptor)
        } catch let error as FinderSemanticAdapterError where error.permitsAXFallback {
            return try fallbackOrThrow(descriptor, error: error)
        }
    }

    private func fallbackOrThrow(
        _ descriptor: FinderSemanticDescriptor,
        error: FinderSemanticAdapterError
    ) throws -> FinderActionResult {
        guard descriptor.capability.supportsAXFallback,
              let axFallback else {
            throw error
        }
        return try axFallback.performAXFallback(for: descriptor)
    }
}

/// The native semantic adapter for the operations that AppKit exposes without
/// scripting Finder. Search remains capability-gated and can safely use AX.
public struct FinderSemanticAdapter: FinderSemanticAdapterProtocol {
    public let capabilityMetadata: FinderCapabilityMetadata

    public init(capabilityMetadata: FinderCapabilityMetadata = .finder) {
        self.capabilityMetadata = capabilityMetadata
    }

    public func descriptor(
        for action: FinderSemanticAction,
        target: FinderSemanticTarget
    ) throws -> FinderSemanticDescriptor {
        try FinderSemanticDescriptor(action: action, target: target, capability: capabilityMetadata)
    }

    public func perform(_ descriptor: FinderSemanticDescriptor) throws -> FinderActionResult {
        guard capabilityMetadata.supports(descriptor.action) else {
            throw FinderSemanticAdapterError.unsupportedAction(descriptor.action)
        }

        switch (descriptor.action, descriptor.target) {
        case (.open, .item(let path)):
            guard NSWorkspace.shared.open(URL(fileURLWithPath: path)) else {
                throw FinderSemanticAdapterError.unavailable("Finder could not open '\(path)'.")
            }
            return FinderActionResult(action: .open, route: .semantic, message: "Finder opened the requested item.")
        case (.select, .item(let path)), (.reveal, .item(let path)):
            let url = URL(fileURLWithPath: path)
            NSWorkspace.shared.activateFileViewerSelecting([url])
            let action: FinderSemanticAction = descriptor.action
            return FinderActionResult(action: action, route: .semantic, message: "Finder selected the requested item.")
        case (.search, .search):
            throw FinderSemanticAdapterError.unsupportedAction(.search)
        default:
            throw FinderSemanticAdapterError.invalidTarget("The Finder descriptor target does not match its action.")
        }
    }
}
