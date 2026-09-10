import Foundation

/// The native automation mechanisms that can be advertised by operate.
///
/// This is a route description, not an instruction to invoke the mechanism.
/// Implementations must keep execution behind a separately reviewed boundary.
public enum AutomationAdapterKind: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case appIntents = "app_intents"
    case shortcuts
    case appleScript = "apple_script"
}

/// Stable capability names exposed by an automation adapter.
///
/// The route and capability are intentionally separate: an App Intent and a
/// Shortcut may both support `execute`, while callers can still discover which
/// mechanism would be used.
public enum AutomationCapability: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case execute
    case observe
    case executeScript = "execute_script"
    case runShortcut = "run_shortcut"
    case invokeAppIntent = "invoke_app_intent"
}

/// Permissions an adapter must already have before it can be selected.
///
/// These values describe the permission boundary; this registry never prompts
/// for or requests a permission.
public enum AutomationPermission: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case automation
    case shortcuts
    case appIntents = "app_intents"
}

/// An explicit application target for native automation.
///
/// At least one identity field is required for route selection. A registry may
/// contain family-wide adapters, but it never guesses the frontmost application.
public struct AutomationTarget: Codable, Equatable, Sendable {
    public let bundleIdentifier: String?
    public let appFamily: String?

    public init(bundleIdentifier: String? = nil, appFamily: String? = nil) {
        self.bundleIdentifier = bundleIdentifier
        self.appFamily = appFamily
    }

    public var hasExplicitIdentity: Bool {
        bundleIdentifier?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
            || appFamily?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
    }
}

/// Versioned, transport-safe metadata for one native automation adapter.
///
/// Metadata is all the registry needs to make a safe preflight decision. It
/// contains no script source, Shortcut input, intent payload, or other secret
/// material, and it is safe to expose during capability discovery.
public struct AutomationAdapterMetadata: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let identifier: String
    public let kind: AutomationAdapterKind
    public let appFamily: String?
    public let bundleIdentifier: String?
    public let capabilities: Set<AutomationCapability>
    public let requiredPermissions: Set<AutomationPermission>
    public let timeout: TimeInterval
    public let isLocal: Bool
    public let isDeterministic: Bool

    public init(
        version: Int = AutomationAdapterMetadata.currentVersion,
        identifier: String,
        kind: AutomationAdapterKind,
        appFamily: String? = nil,
        bundleIdentifier: String? = nil,
        capabilities: Set<AutomationCapability>,
        requiredPermissions: Set<AutomationPermission> = [],
        timeout: TimeInterval,
        isLocal: Bool = true,
        isDeterministic: Bool = true
    ) {
        self.version = version
        self.identifier = identifier
        self.kind = kind
        self.appFamily = appFamily
        self.bundleIdentifier = bundleIdentifier
        self.capabilities = capabilities
        self.requiredPermissions = requiredPermissions
        self.timeout = timeout
        self.isLocal = isLocal
        self.isDeterministic = isDeterministic
    }

    /// String views are useful to generic capability/profile consumers.
    public var capabilityNames: Set<String> {
        Set(capabilities.map(\.rawValue))
    }

    public var permissionNames: Set<String> {
        Set(requiredPermissions.map(\.rawValue))
    }

    /// Compatibility spellings for clients that call these fields capabilities
    /// and permissions rather than requirements.
    public var permissions: Set<AutomationPermission> { requiredPermissions }
    public var localOnly: Bool { isLocal }
    public var deterministic: Bool { isDeterministic }

    public func supports(_ capability: AutomationCapability) -> Bool {
        capabilities.contains(capability)
    }

    /// A non-nil value means this metadata is unsafe to use as an execution route.
    public var safetyIssue: String? {
        guard !identifier.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return "adapter identifier is empty"
        }
        guard version == Self.currentVersion else {
            return "adapter metadata version is unsupported"
        }
        guard !capabilities.isEmpty else {
            return "adapter advertises no capabilities"
        }
        guard timeout > 0, timeout.isFinite else {
            return "adapter timeout must be finite and greater than zero"
        }
        guard isLocal else {
            return "adapter is not local"
        }
        guard isDeterministic else {
            return "adapter is not deterministic"
        }
        return nil
    }

    public var isSafe: Bool { safetyIssue == nil }

    /// Built-in metadata records. These describe routes only; they do not
    /// construct or invoke any Apple framework automation object.
    public static let appIntents = AutomationAdapterMetadata(
        identifier: "app-intents",
        kind: .appIntents,
        capabilities: [.invokeAppIntent],
        timeout: 10
    )

    public static let shortcuts = AutomationAdapterMetadata(
        identifier: "shortcuts",
        kind: .shortcuts,
        capabilities: [.runShortcut],
        requiredPermissions: [.shortcuts],
        timeout: 30
    )

    public static let appleScript = AutomationAdapterMetadata(
        identifier: "apple-script",
        kind: .appleScript,
        capabilities: [.executeScript],
        requiredPermissions: [.automation],
        timeout: 10
    )
}

/// The metadata-only adapter seam. No method here executes an external app.
///
/// A production integration can conform with a separately reviewed executor,
/// while tests and capability discovery use this contract without touching
/// NSAppleScript, Shortcuts, App Intents, or a live application.
public protocol AutomationAdapter: Sendable {
    var metadata: AutomationAdapterMetadata { get }
}

/// A simple metadata-only adapter useful for built-in routes and tests.
public struct MetadataAutomationAdapter: AutomationAdapter, Sendable {
    public let metadata: AutomationAdapterMetadata

    public init(metadata: AutomationAdapterMetadata) {
        self.metadata = metadata
    }
}

/// Safe, deterministic outcome of a registry preflight.
public enum AutomationRouteDecisionStatus: String, Codable, Equatable, Sendable {
    case selected
    case refused
}

/// One reason an adapter was not selected. Reasons contain metadata only and
/// never include script text, Shortcut arguments, or intent payloads.
public struct AutomationRouteRejection: Codable, Equatable, Sendable {
    public let adapterIdentifier: String
    public let route: AutomationAdapterKind
    public let reason: String

    public init(adapterIdentifier: String, route: AutomationAdapterKind, reason: String) {
        self.adapterIdentifier = adapterIdentifier
        self.route = route
        self.reason = reason
    }
}

/// Structured route-selection evidence returned before any adapter execution.
public struct AutomationRouteDecision: Codable, Equatable, Sendable {
    public let status: AutomationRouteDecisionStatus
    public let route: AutomationAdapterKind?
    public let adapterIdentifier: String?
    public let timeout: TimeInterval?
    public let reason: String
    public let rejections: [AutomationRouteRejection]

    public init(
        status: AutomationRouteDecisionStatus,
        route: AutomationAdapterKind?,
        adapterIdentifier: String?,
        timeout: TimeInterval? = nil,
        reason: String,
        rejections: [AutomationRouteRejection] = []
    ) {
        self.status = status
        self.route = route
        self.adapterIdentifier = adapterIdentifier
        self.timeout = timeout
        self.reason = reason
        self.rejections = rejections
    }

    public static func selected(metadata: AutomationAdapterMetadata) -> Self {
        Self(
            status: .selected,
            route: metadata.kind,
            adapterIdentifier: metadata.identifier,
            timeout: metadata.timeout,
            reason: "Adapter selected after capability, target, permission, and safety checks."
        )
    }

    public static func refused(reason: String, rejections: [AutomationRouteRejection] = []) -> Self {
        Self(
            status: .refused,
            route: nil,
            adapterIdentifier: nil,
            reason: reason,
            rejections: rejections
        )
    }

    public var isExecutable: Bool {
        status == .selected && route != nil && adapterIdentifier != nil && timeout != nil
    }
}

/// Registry for metadata-only AppleScript, Shortcuts, and App Intents routes.
///
/// Selection is fail-closed: callers must provide an explicit target and the
/// complete set of already-granted permissions. Registration order cannot alter
/// the result; routes are sorted by the conservative native ordering below.
public struct AutomationAdapterRegistry: Sendable {
    public static let safeRouteOrdering: [AutomationAdapterKind] = [
        .appIntents,
        .shortcuts,
        .appleScript,
    ]

    public let adapters: [any AutomationAdapter]

    public init(adapters: [any AutomationAdapter] = []) {
        self.adapters = adapters
    }

    /// The built-in route catalog. These adapters only advertise metadata.
    public static let standard = AutomationAdapterRegistry(adapters: [
        MetadataAutomationAdapter(metadata: .appIntents),
        MetadataAutomationAdapter(metadata: .shortcuts),
        MetadataAutomationAdapter(metadata: .appleScript),
    ])

    /// Compatibility spelling for callers that call the built-in catalog default.
    public static let builtIn = standard

    /// Returns every registered metadata record in stable route order.
    public func discover() -> [AutomationAdapterMetadata] {
        sortedAdapters.map(\.metadata)
    }

    /// Returns metadata that can target the explicit application identity.
    public func discover(for target: AutomationTarget) -> [AutomationAdapterMetadata] {
        guard target.hasExplicitIdentity else { return [] }
        return sortedAdapters
            .filter { targetMatches($0.metadata, target: target) }
            .map(\.metadata)
    }

    /// Selects one route without invoking it or prompting for permissions.
    public func select(
        capability: AutomationCapability,
        for target: AutomationTarget,
        grantedPermissions: Set<AutomationPermission> = []
    ) -> AutomationRouteDecision {
        guard target.hasExplicitIdentity else {
            return .refused(reason: "Automation route selection requires an explicit target identity.")
        }

        var rejections: [AutomationRouteRejection] = []
        for adapter in sortedAdapters {
            let metadata = adapter.metadata
            let rejection: String?
            if let safetyIssue = metadata.safetyIssue {
                rejection = safetyIssue
            } else if !metadata.supports(capability) {
                rejection = "adapter does not advertise the requested capability"
            } else if !targetMatches(metadata, target: target) {
                rejection = "adapter does not match the requested target"
            } else {
                let missingPermissions = metadata.requiredPermissions.subtracting(grantedPermissions)
                rejection = missingPermissions.isEmpty
                    ? nil
                    : "required permission is not granted: " + missingPermissions
                        .map(\.rawValue)
                        .sorted()
                        .joined(separator: ", ")
            }

            if let rejection {
                rejections.append(AutomationRouteRejection(
                    adapterIdentifier: metadata.identifier,
                    route: metadata.kind,
                    reason: rejection
                ))
            } else {
                return .selected(metadata: metadata)
            }
        }

        return .refused(
            reason: "No safe local automation adapter is available for the requested capability and target.",
            rejections: rejections
        )
    }

    private var sortedAdapters: [any AutomationAdapter] {
        adapters.sorted { lhs, rhs in
            let lhsOrder = Self.safeRouteOrdering.firstIndex(of: lhs.metadata.kind) ?? Self.safeRouteOrdering.count
            let rhsOrder = Self.safeRouteOrdering.firstIndex(of: rhs.metadata.kind) ?? Self.safeRouteOrdering.count
            if lhsOrder != rhsOrder { return lhsOrder < rhsOrder }
            return lhs.metadata.identifier < rhs.metadata.identifier
        }
    }

    private func targetMatches(_ metadata: AutomationAdapterMetadata, target: AutomationTarget) -> Bool {
        if let bundleIdentifier = metadata.bundleIdentifier {
            guard let targetBundleIdentifier = target.bundleIdentifier,
                  bundleIdentifier == targetBundleIdentifier else { return false }
        }
        if let appFamily = metadata.appFamily {
            guard let targetAppFamily = target.appFamily,
                  appFamily.localizedCaseInsensitiveCompare(targetAppFamily) == .orderedSame else { return false }
        }
        return true
    }
}

/// Compatibility aliases for callers that use descriptor/route terminology.
public typealias AutomationAdapterDescriptor = AutomationAdapterMetadata
public typealias AutomationAdapterRoute = AutomationAdapterKind
public typealias AutomationPermissionRequirement = AutomationPermission
public typealias AutomationRouteSelection = AutomationRouteDecision
