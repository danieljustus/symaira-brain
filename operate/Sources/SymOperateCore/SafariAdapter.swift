import Foundation

/// Capabilities exposed by the Safari semantic contract.
///
/// These names are intentionally stable so the descriptor can be surfaced to
/// clients without making them depend on Swift enum names.
public enum SafariSemanticCapability: String, Codable, CaseIterable, Hashable, Sendable {
    case tabIdentity = "tab_identity"
    case tabSelection = "tab_selection"
    case addressFieldFocus = "address_field_focus"
    case navigation = "navigation"
    case loadingState = "loading_state"
}

/// A versioned description of the Safari-specific route.
public struct SafariSemanticDescriptor: Codable, Equatable, Sendable {
    public static let currentVersion = 1
    public static let bundleID = "com.apple.Safari"

    public let version: Int
    public let appFamily: String
    public let bundleIdentifier: String
    public let capabilities: Set<SafariSemanticCapability>

    public init(
        version: Int = SafariSemanticDescriptor.currentVersion,
        appFamily: String = "safari",
        bundleIdentifier: String = SafariSemanticDescriptor.bundleID,
        capabilities: Set<SafariSemanticCapability> = Set(SafariSemanticCapability.allCases)
    ) {
        self.version = version
        self.appFamily = appFamily
        self.bundleIdentifier = bundleIdentifier
        self.capabilities = capabilities
    }

    public static let safari = SafariSemanticDescriptor()

    public func supports(_ capability: SafariSemanticCapability) -> Bool {
        capabilities.contains(capability)
    }

    /// The descriptor can be consumed by the generic capability-profile store.
    public var capabilityProfile: CapabilityProfile {
        CapabilityProfile(appFamily: appFamily, capabilities: Set(capabilities.map(\.rawValue)))
    }
}

/// Stable identity and state for one Safari tab.
public struct SafariTabState: Codable, Equatable, Sendable {
    public let id: String
    public let index: Int
    public let title: String?
    public let url: String?
    public let isSelected: Bool
    public let isLoading: Bool?

    public init(
        id: String,
        index: Int,
        title: String? = nil,
        url: String? = nil,
        isSelected: Bool = false,
        isLoading: Bool? = nil
    ) {
        self.id = id
        self.index = index
        self.title = title
        self.url = url
        self.isSelected = isSelected
        self.isLoading = isLoading
    }
}

/// State for a Safari window, including its tab dimension when available.
public struct SafariWindowState: Codable, Equatable, Sendable {
    public let windowID: Int
    public let title: String?
    public let isFocused: Bool
    public let tabs: [SafariTabState]

    public init(windowID: Int, title: String? = nil, isFocused: Bool = false, tabs: [SafariTabState] = []) {
        self.windowID = windowID
        self.title = title
        self.isFocused = isFocused
        self.tabs = tabs
    }

    public var selectedTab: SafariTabState? {
        tabs.first(where: \.isSelected)
    }
}

/// A redacted, post-action observation of Safari.
///
/// `addressFieldValue` is deliberately not part of the contract: navigation
/// can be verified from the selected tab URL without exposing typed address
/// bar contents (which may contain credentials or other secrets).
public struct SafariObservation: Codable, Equatable, Sendable {
    public let processID: Int32?
    public let bundleIdentifier: String?
    public let windows: [SafariWindowState]
    public let focusedWindowID: Int?
    public let addressFieldFocused: Bool?
    public let observedAt: String

    public init(
        processID: Int32? = nil,
        bundleIdentifier: String? = SafariSemanticDescriptor.bundleID,
        windows: [SafariWindowState] = [],
        focusedWindowID: Int? = nil,
        addressFieldFocused: Bool? = nil,
        observedAt: String = DateFormats.iso8601String(from: Date())
    ) {
        self.processID = processID
        self.bundleIdentifier = bundleIdentifier
        self.windows = windows
        self.focusedWindowID = focusedWindowID
        self.addressFieldFocused = addressFieldFocused
        self.observedAt = observedAt
    }

    public var selectedTab: SafariTabState? {
        guard let windowID = focusedWindowID else {
            return windows.first(where: { $0.isFocused })?.selectedTab
        }
        return windows.first(where: { $0.windowID == windowID })?.selectedTab
    }
}

/// The intentionally small set of safe Safari actions.
///
/// There is no arbitrary AX action in this enum. In particular, password,
/// payment, permission and account-recovery controls cannot be reached through
/// this adapter.
public enum SafariSemanticAction: Equatable, Sendable {
    case selectTab(id: String, windowID: Int?)
    case focusAddressField
    case navigate(to: String)

    public var capability: SafariSemanticCapability {
        switch self {
        case .selectTab: return .tabSelection
        case .focusAddressField: return .addressFieldFocus
        case .navigate: return .navigation
        }
    }

    public var name: String {
        switch self {
        case .selectTab: return "select_tab"
        case .focusAddressField: return "focus_address_field"
        case .navigate: return "navigate"
        }
    }
}

/// The route used for a Safari operation.
public enum SafariExecutionRoute: String, Codable, Sendable {
    case semantic
    case accessibilityFallback = "accessibility_fallback"
}

/// The verified outcome of one Safari semantic action.
public enum SafariActionStatus: String, Codable, Sendable {
    case completed
    case timedOut = "timed_out"
    case suspectedNoop = "suspected_noop"
}

public struct SafariActionResult: Codable, Equatable, Sendable {
    public let action: String
    public let route: SafariExecutionRoute
    public let status: SafariActionStatus
    public let observation: SafariObservation
    public let message: String

    public init(
        action: String,
        route: SafariExecutionRoute,
        status: SafariActionStatus,
        observation: SafariObservation,
        message: String
    ) {
        self.action = action
        self.route = route
        self.status = status
        self.observation = observation
        self.message = message
    }
}

/// A semantic implementation supplied by a future native Safari route.
public protocol SafariSemanticOperationsProtocol: Sendable {
    func observe(target: TargetIdentity) throws -> SafariObservation
    func perform(_ action: SafariSemanticAction, target: TargetIdentity) throws -> SafariObservation
}

/// A generic AX implementation supplied by the existing accessibility layer.
///
/// Keeping this seam as a protocol makes all adapter tests deterministic and
/// prevents tests from launching or attaching to a real Safari process.
public protocol SafariAXFallbackProtocol: Sendable {
    func observe(target: TargetIdentity) throws -> SafariObservation
    func perform(_ action: SafariSemanticAction, target: TargetIdentity) throws -> SafariObservation
}

/// Public contract for a Safari-aware adapter.
public protocol SafariSemanticAdapterProtocol: Sendable {
    var descriptor: SafariSemanticDescriptor { get }
    func observe(target: TargetIdentity) throws -> SafariObservation
    func selectTab(id: String, windowID: Int?, target: TargetIdentity) throws -> SafariActionResult
    func focusAddressField(target: TargetIdentity) throws -> SafariActionResult
    func navigate(to url: String, target: TargetIdentity, timeout: TimeInterval) throws -> SafariActionResult
}

/// Routes supported operations to the semantic implementation and falls back
/// to generic Accessibility when that capability is not available.
public struct SafariSemanticAdapter: SafariSemanticAdapterProtocol, Sendable {
    public let descriptor: SafariSemanticDescriptor
    private let semantic: (any SafariSemanticOperationsProtocol)?
    private let axFallback: any SafariAXFallbackProtocol

    public init(
        descriptor: SafariSemanticDescriptor = .safari,
        semantic: (any SafariSemanticOperationsProtocol)?,
        axFallback: any SafariAXFallbackProtocol
    ) {
        self.descriptor = descriptor
        self.semantic = semantic
        self.axFallback = axFallback
    }

    public func observe(target: TargetIdentity) throws -> SafariObservation {
        try route(for: .tabIdentity) {
            try semantic?.observe(target: target) ?? axFallback.observe(target: target)
        } fallback: {
            try axFallback.observe(target: target)
        }
    }

    public func selectTab(id: String, windowID: Int? = nil, target: TargetIdentity) throws -> SafariActionResult {
        guard !id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AutomationError.invalidArgument("Safari tab id must not be empty.")
        }
        let action = SafariSemanticAction.selectTab(id: id, windowID: windowID)
        let (observation, route) = try execute(action, target: target)
        let selected = observation.windows
            .first(where: { windowID == nil || $0.windowID == windowID })?
            .tabs
            .first(where: { $0.id == id && $0.isSelected })
        let status: SafariActionStatus = selected == nil ? .suspectedNoop : .completed
        return result(for: action, route: route, status: status, observation: observation)
    }

    public func focusAddressField(target: TargetIdentity) throws -> SafariActionResult {
        let action = SafariSemanticAction.focusAddressField
        let (observation, route) = try execute(action, target: target)
        let status: SafariActionStatus = observation.addressFieldFocused == true ? .completed : .suspectedNoop
        return result(for: action, route: route, status: status, observation: observation)
    }

    public func navigate(to url: String, target: TargetIdentity, timeout: TimeInterval = 10) throws -> SafariActionResult {
        guard timeout > 0, timeout.isFinite else {
            throw AutomationError.invalidArgument("Safari navigation timeout must be finite and greater than zero.")
        }
        let normalizedURL = try Self.validateNavigationURL(url)
        let action = SafariSemanticAction.navigate(to: normalizedURL)
        let (observation, route) = try execute(action, target: target)
        let selectedTab = observation.selectedTab
        let reachedURL = selectedTab?.url == normalizedURL
        let stillLoading = selectedTab?.isLoading == true
        let status: SafariActionStatus
        if reachedURL && !stillLoading {
            status = .completed
        } else if stillLoading {
            status = .timedOut
        } else {
            status = .suspectedNoop
        }
        return result(for: action, route: route, status: status, observation: observation)
    }

    private func execute(
        _ action: SafariSemanticAction,
        target: TargetIdentity
    ) throws -> (SafariObservation, SafariExecutionRoute) {
        try validateSafariTarget(target)
        guard let semantic, descriptor.supports(action.capability) else {
            return (try axFallback.perform(action, target: target), .accessibilityFallback)
        }
        // Do not retry a failed semantic action through AX: repeating a
        // navigation or tab change could create an unverified duplicate effect.
        return (try semantic.perform(action, target: target), .semantic)
    }

    private func route<T>(
        for capability: SafariSemanticCapability,
        semantic: () throws -> T,
        fallback: () throws -> T
    ) throws -> T {
        guard self.semantic != nil, descriptor.supports(capability) else {
            return try fallback()
        }
        return try semantic()
    }

    private func result(
        for action: SafariSemanticAction,
        route: SafariExecutionRoute,
        status: SafariActionStatus,
        observation: SafariObservation
    ) -> SafariActionResult {
        let message: String
        switch status {
        case .completed:
            message = "Safari \(action.name) completed and was verified from observed state."
        case .timedOut:
            message = "Safari navigation timed out before the observed page finished loading."
        case .suspectedNoop:
            message = "Safari \(action.name) was submitted but its expected state was not observed."
        }
        return SafariActionResult(
            action: action.name,
            route: route,
            status: status,
            observation: observation,
            message: message
        )
    }

    private func validateSafariTarget(_ target: TargetIdentity) throws {
        if let bundleID = target.bundleID, !bundleID.isEmpty, bundleID != descriptor.bundleIdentifier {
            throw AutomationError.targetMismatch("The requested target is not Safari.")
        }
        if let appName = target.appName, !appName.isEmpty,
           appName.localizedCaseInsensitiveCompare("Safari") != .orderedSame {
            throw AutomationError.targetMismatch("The requested target is not Safari.")
        }
    }

    private static func validateNavigationURL(_ rawURL: String) throws -> String {
        let value = rawURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let components = URLComponents(string: value),
              let scheme = components.scheme?.lowercased(),
              ["http", "https"].contains(scheme),
              components.host?.isEmpty == false,
              components.user == nil,
              components.password == nil
        else {
            throw AutomationError.invalidArgument("Safari navigation requires an http(s) URL without embedded credentials.")
        }
        return value
    }
}

private extension SafariSemanticAction {
    var actionName: String { name }
}
