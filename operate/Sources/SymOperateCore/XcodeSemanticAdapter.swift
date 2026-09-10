import Foundation

/// The bounded semantic operations exposed by Xcode's native automation route.
///
/// The enum intentionally contains no arbitrary script or AX operation. Callers
/// provide operation parameters in `XcodeActionRequest`, which keeps the action
/// vocabulary stable and makes capability checks explicit.
public enum XcodeSemanticAction: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case openProject = "open_project"
    case openDocument = "open_document"
    case build
    case test
    case navigate

    /// Compatibility spellings used by clients that name the operation rather
    /// than the capability it exercises.
    public static let buildProject = Self.build
    public static let runTests = Self.test
    public static let navigateTo = Self.navigate

    public var capability: XcodeCapability {
        switch self {
        case .openProject: return .project
        case .openDocument: return .document
        case .build: return .build
        case .test: return .test
        case .navigate: return .navigation
        }
    }
}

/// Capability areas advertised by an Xcode descriptor.
public enum XcodeCapability: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case project
    case document
    case build
    case test
    case navigation
}

/// Compatibility spelling matching the semantic action terminology.
public typealias XcodeSemanticCapability = XcodeCapability

/// Versioned metadata for Xcode's semantic route.
///
/// This is descriptive metadata only. It never launches Xcode and never performs
/// an operation by itself.
public struct XcodeSemanticDescriptor: Codable, Equatable, Sendable {
    public static let currentVersion = 1
    public static let bundleID = "com.apple.dt.Xcode"

    public let version: Int
    public let appFamily: String
    public let bundleIdentifier: String
    public let capabilities: Set<XcodeCapability>
    public let semanticActions: Set<XcodeSemanticAction>
    public let supportsAXFallback: Bool

    public init(
        version: Int = XcodeSemanticDescriptor.currentVersion,
        appFamily: String = "xcode",
        bundleIdentifier: String = XcodeSemanticDescriptor.bundleID,
        capabilities: Set<XcodeCapability> = Set(XcodeCapability.allCases),
        semanticActions: Set<XcodeSemanticAction> = Set(XcodeSemanticAction.allCases),
        supportsAXFallback: Bool = true
    ) {
        self.version = version
        self.appFamily = appFamily
        self.bundleIdentifier = bundleIdentifier
        self.capabilities = capabilities
        self.semanticActions = semanticActions
        self.supportsAXFallback = supportsAXFallback
    }

    /// Conservative metadata for Apple's Xcode application.
    public static let xcode = XcodeSemanticDescriptor()
    public static let appleXcode = XcodeSemanticDescriptor.xcode

    public var actions: Set<XcodeSemanticAction> { semanticActions }

    public var supportsProject: Bool { supports(XcodeCapability.project) }
    public var supportsDocument: Bool { supports(XcodeCapability.document) }
    public var supportsBuild: Bool { supports(XcodeCapability.build) }
    public var supportsTest: Bool { supports(XcodeCapability.test) }
    public var supportsNavigation: Bool { supports(XcodeCapability.navigation) }

    public func supports(_ action: XcodeSemanticAction) -> Bool {
        semanticActions.contains(action)
    }

    public func supports(_ capability: XcodeCapability) -> Bool {
        capabilities.contains(capability)
    }

    /// String names are useful to generic capability/profile consumers.
    public var capabilityNames: Set<String> {
        Set(capabilities.map(\.rawValue))
    }

    public var capabilityProfile: CapabilityProfile {
        CapabilityProfile(appFamily: appFamily, capabilities: capabilityNames)
    }
}

/// Compatibility spelling matching the other native application adapters.
public typealias XcodeCapabilityDescriptor = XcodeSemanticDescriptor

/// Identifies Xcode and the routes it advertises.
public struct XcodeDescriptor: Codable, Equatable, Sendable {
    public let bundleIdentifier: String
    public let applicationName: String
    public let capabilityMetadata: XcodeSemanticDescriptor

    public init(
        bundleIdentifier: String = XcodeSemanticDescriptor.bundleID,
        applicationName: String = "Xcode",
        capabilityMetadata: XcodeSemanticDescriptor = .xcode
    ) {
        self.bundleIdentifier = bundleIdentifier
        self.applicationName = applicationName
        self.capabilityMetadata = capabilityMetadata
    }

    public static let xcode = XcodeDescriptor()

    public var capabilities: XcodeSemanticDescriptor { capabilityMetadata }
}

/// Parameters for one bounded Xcode operation.
///
/// Paths, schemes, test names, and navigation targets are data passed to an
/// injected semantic implementation. They are never interpolated into a shell
/// command or AppleScript by this contract.
public struct XcodeActionRequest: Codable, Equatable, Sendable {
    public let action: XcodeSemanticAction
    public let projectPath: String?
    public let documentPath: String?
    public let scheme: String?
    public let testName: String?
    public let destination: String?
    public let navigationTarget: String?
    public let snapshotID: String?
    public let elementID: String?
    public let axAction: String

    public init(
        action: XcodeSemanticAction,
        projectPath: String? = nil,
        documentPath: String? = nil,
        scheme: String? = nil,
        testName: String? = nil,
        destination: String? = nil,
        navigationTarget: String? = nil,
        snapshotID: String? = nil,
        elementID: String? = nil,
        axAction: String = "AXPress"
    ) {
        self.action = action
        self.projectPath = projectPath
        self.documentPath = documentPath
        self.scheme = scheme
        self.testName = testName
        self.destination = destination
        self.navigationTarget = navigationTarget
        self.snapshotID = snapshotID
        self.elementID = elementID
        self.axAction = axAction
    }

    /// Compatibility spelling for callers that call a file a workspace path.
    public var workspacePath: String? { projectPath }

    /// AX fallback is only safe when it refers to a freshly observed element.
    public var hasAXReference: Bool {
        snapshotID?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
            && elementID?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
    }
}

public enum XcodeActionRoute: String, Codable, Equatable, Sendable {
    case semantic
    case ax

    public static let accessibilityFallback = Self.ax
}

public struct XcodeActionResult: Codable, Equatable, Sendable {
    public let action: XcodeSemanticAction
    public let route: XcodeActionRoute
    public let message: String

    public init(action: XcodeSemanticAction, route: XcodeActionRoute, message: String) {
        self.action = action
        self.route = route
        self.message = message
    }
}

/// A native implementation supplied by the Xcode integration layer.
///
/// Keeping the side effects behind this protocol makes descriptor and routing
/// tests pure; production can later provide a native implementation without
/// changing the safety contract.
public protocol XcodeSemanticOperationsProtocol: Sendable {
    func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult
}

/// Boundary to the existing Accessibility service.
///
/// Implementations must use an explicitly observed snapshot/element pair. This
/// contract deliberately does not search Xcode's UI or guess a control.
public protocol XcodeAXFallbackProtocol: Sendable {
    func performAX(_ request: XcodeActionRequest) throws -> XcodeActionResult
}

/// Public contract for an Xcode semantic adapter.
public protocol XcodeSemanticAdapterProtocol: Sendable {
    var descriptor: XcodeDescriptor { get }
    func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult
}

/// Routes Xcode requests through the semantic implementation first and uses AX
/// only for capability/unavailability cases.
///
/// A real semantic operation failure is never retried through AX: doing so could
/// duplicate a build, test, document mutation, or navigation action.
public struct XcodeActionRouter: Sendable {
    public let semantic: any XcodeSemanticAdapterProtocol
    public let axFallback: any XcodeAXFallbackProtocol

    public init(
        semantic: any XcodeSemanticAdapterProtocol,
        axFallback: any XcodeAXFallbackProtocol
    ) {
        self.semantic = semantic
        self.axFallback = axFallback
    }

    public func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult {
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

    private func performAXFallback(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        guard semantic.descriptor.capabilityMetadata.supportsAXFallback else {
            throw AutomationError.unsupported(
                "No supported route is available for Xcode action '\(request.action.rawValue)'."
            )
        }
        guard request.hasAXReference else {
            throw AutomationError.invalidArgument(
                "AX fallback for Xcode action '\(request.action.rawValue)' requires snapshot_id and element_id."
            )
        }
        return try axFallback.performAX(request)
    }
}

/// The default adapter shell. Native operations are injected so this type is
/// safe to construct in tests and remains inert when no native route exists.
public struct XcodeSemanticAdapter: XcodeSemanticAdapterProtocol, Sendable {
    public let descriptor: XcodeDescriptor
    private let operations: (any XcodeSemanticOperationsProtocol)?

    public init(
        descriptor: XcodeDescriptor = .xcode,
        operations: (any XcodeSemanticOperationsProtocol)? = nil
    ) {
        self.descriptor = descriptor
        self.operations = operations
    }

    public func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        guard descriptor.capabilityMetadata.supports(request.action) else {
            throw AutomationError.unsupported(
                "Xcode semantic action '\(request.action.rawValue)' is unsupported."
            )
        }
        guard let operations else {
            throw AutomationError.unavailable("The native Xcode semantic route is unavailable.")
        }
        return try operations.perform(request)
    }
}

/// Concrete bridge from the adapter contract to the existing AX element cache.
public struct AccessibilityXcodeFallback: XcodeAXFallbackProtocol, @unchecked Sendable {
    private let accessibility: any AccessibilityServiceProtocol

    public init(accessibility: any AccessibilityServiceProtocol) {
        self.accessibility = accessibility
    }

    public func performAX(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        guard let snapshotID = request.snapshotID, let elementID = request.elementID else {
            throw AutomationError.invalidArgument("AX fallback requires snapshot_id and element_id.")
        }
        try accessibility.performElementAction(
            snapshotID: snapshotID,
            elementID: elementID,
            action: request.axAction
        )
        return XcodeActionResult(
            action: request.action,
            route: .ax,
            message: "Xcode action routed through Accessibility (\(request.axAction))."
        )
    }
}

/// Compatibility aliases for callers using the shorter adapter terminology.
public typealias XcodeAdapterProtocol = XcodeSemanticAdapterProtocol
public typealias AXXcodeFallback = AccessibilityXcodeFallback
