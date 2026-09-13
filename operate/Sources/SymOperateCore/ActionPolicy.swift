import Foundation

/// Granular permission flags for automation actions.
///
/// Each flag represents a category of automation operation that can be
/// independently allowed or denied. Used by `ActionPolicy` and surfaced
/// in safety refusals so agents can classify denials without string-matching.
public struct PermissionFlags: OptionSet, Codable, Sendable {
    public let rawValue: UInt8

    public init(rawValue: UInt8) {
        self.rawValue = rawValue
    }

    public static let capture           = PermissionFlags(rawValue: 1 << 0)
    public static let input             = PermissionFlags(rawValue: 1 << 1)
    public static let appControl        = PermissionFlags(rawValue: 1 << 2)
    public static let menuAction        = PermissionFlags(rawValue: 1 << 3)
    /// Classification for a refusal by the destructive-keyword guard. This is a
    /// refusal *code*, not a grantable permission: the guard refuses whether or
    /// not it appears in a grant, so it is deliberately absent from ``all`` and
    /// from ``allFlagNames``. Bit 5 stayed with the equally ungrantable
    /// `secure_field_access` and is not reused, so a stored raw value keeps its
    /// meaning.
    public static let destructiveAction = PermissionFlags(rawValue: 1 << 4)
    public static let policyModify      = PermissionFlags(rawValue: 1 << 6)
    /// Every permission an operator can actually grant or withhold — the flags
    /// an action is gated on. Withholding any of these changes what automation
    /// may do; that is what makes them worth advertising.
    public static let all: PermissionFlags = [
        .capture, .input, .appControl, .menuAction, .policyModify,
    ]
}

/// Identity dimensions used to select a least-privilege policy grant.
public struct PolicyScope: Codable, Equatable, Hashable, Sendable {
    public let agent: String?
    public let application: String?
    public let workflow: String?

    public init(agent: String? = nil, application: String? = nil, workflow: String? = nil) {
        self.agent = agent
        self.application = application
        self.workflow = workflow
    }

    public init(agentID: String? = nil, applicationID: String? = nil, workflowID: String? = nil) {
        self.init(agent: agentID, application: applicationID, workflow: workflowID)
    }

    public var agentID: String? { agent }
    public var applicationID: String? { application }
    public var workflowID: String? { workflow }

    public var isEmpty: Bool {
        agent == nil && application == nil && workflow == nil
    }
}

/// A permission ceiling for one or more caller scope dimensions.
public struct ScopedPermissionGrant: Codable, Equatable, Sendable {
    public let scope: PolicyScope
    public let permissions: PermissionFlags

    public init(scope: PolicyScope, permissions: PermissionFlags) {
        self.scope = scope
        self.permissions = permissions
    }
}

/// Compatibility spelling for callers that refer to a scoped grant directly.
public typealias ScopedGrant = ScopedPermissionGrant

public struct ActionPolicy: Codable, Sendable {
    public var extraDenyKeywords: Set<String>
    public var allowedKeywords: Set<String>
    /// The effective permissions currently granted to the calling agent.
    /// This can only be narrowed within `startupGrantedPermissions`.
    public private(set) var grantedPermissions: PermissionFlags
    /// The immutable ceiling established when the process started.
    public let startupGrantedPermissions: PermissionFlags
    /// Optional least-privilege ceilings selected by caller, application and workflow.
    /// Every applicable grant is intersected with the startup ceiling.
    public var scopedGrants: [ScopedPermissionGrant]

    public init(
        extraDenyKeywords: Set<String> = [],
        allowedKeywords: Set<String> = [],
        grantedPermissions: PermissionFlags = .all,
        startupGrantedPermissions: PermissionFlags? = nil,
        scopedGrants: [ScopedPermissionGrant] = []
    ) {
        self.extraDenyKeywords = extraDenyKeywords
        self.allowedKeywords = allowedKeywords
        let startup = startupGrantedPermissions ?? grantedPermissions
        self.startupGrantedPermissions = startup
        self.grantedPermissions = grantedPermissions.intersection(startup)
        self.scopedGrants = scopedGrants
    }

    private enum CodingKeys: String, CodingKey {
        case extraDenyKeywords
        case allowedKeywords
        case grantedPermissions
        case startupGrantedPermissions
        case scopedGrants
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let granted = try container.decodeIfPresent(PermissionFlags.self, forKey: .grantedPermissions) ?? .all
        let startup = try container.decodeIfPresent(PermissionFlags.self, forKey: .startupGrantedPermissions) ?? granted
        self.extraDenyKeywords = try container.decodeIfPresent(Set<String>.self, forKey: .extraDenyKeywords) ?? []
        self.allowedKeywords = try container.decodeIfPresent(Set<String>.self, forKey: .allowedKeywords) ?? []
        self.startupGrantedPermissions = startup
        self.grantedPermissions = granted.intersection(startup)
        self.scopedGrants = try container.decodeIfPresent([ScopedPermissionGrant].self, forKey: .scopedGrants) ?? []
    }

    /// Resolves the effective grant using strict intersection semantics.
    ///
    /// A configured dimension must be present in the request and match at least
    /// one grant. This makes omitted or unknown caller identity fail closed.
    public func effectivePermissions(for scope: PolicyScope? = nil) -> PermissionFlags {
        var effective = grantedPermissions.intersection(startupGrantedPermissions)
        guard !scopedGrants.isEmpty else { return effective }
        let requested = scope ?? PolicyScope(agent: nil, application: nil, workflow: nil)
        let configuredAgents = scopedGrants.contains { $0.scope.agent != nil }
        let configuredApplications = scopedGrants.contains { $0.scope.application != nil }
        let configuredWorkflows = scopedGrants.contains { $0.scope.workflow != nil }

        if configuredAgents {
            guard let agent = requested.agent,
                  scopedGrants.contains(where: { $0.scope.agent == agent }) else { return [] }
        }
        if configuredApplications {
            guard let application = requested.application,
                  scopedGrants.contains(where: { $0.scope.application == application }) else { return [] }
        }
        if configuredWorkflows {
            guard let workflow = requested.workflow,
                  scopedGrants.contains(where: { $0.scope.workflow == workflow }) else { return [] }
        }

        for grant in scopedGrants where grant.scope.matches(requested) {
            effective.formIntersection(grant.permissions)
        }
        return effective
    }

    /// Replaces the effective grant only when it is a subset of the startup grant.
    @discardableResult
    public mutating func setGrantedPermissions(_ requested: PermissionFlags) -> Bool {
        guard requested.subtracting(startupGrantedPermissions).isEmpty else { return false }
        grantedPermissions = requested
        return true
    }

    static let defaultDenyKeywords: Set<String> = [
        "delete", "remove", "erase", "clear", "trash",
        "uninstall", "allow", "authorize", "unlock",
        "quit", "terminate", "force quit", "shutdown"
    ]

    /// Returns the first violated permission flag, or `nil` if the action is permitted.
    ///
    /// Evaluates keyword-based deny/allow rules first (backward-compatible),
    /// then checks the granted permissions for the relevant operation category.
    public func firstViolation(
        role: String? = nil, title: String? = nil, label: String? = nil, value: String? = nil,
        requiredPermission: PermissionFlags? = nil,
        scope: PolicyScope? = nil
    ) -> PermissionFlags? {
        let allDenyKeywords = Self.defaultDenyKeywords.union(extraDenyKeywords)
        let inputs = [role, title, label, value].compactMap { $0?.lowercased() }
        for input in inputs {
            for keyword in allDenyKeywords {
                if input.contains(keyword) {
                    let keywordBase = String(keyword.prefix(while: { $0 != " " }))
                    if allowedKeywords.contains(keywordBase) {
                        continue
                    }
                    return .destructiveAction
                }
            }
        }

        // Check specific permission if requested.
        if let requiredPermission, !effectivePermissions(for: scope).contains(requiredPermission) {
            return requiredPermission
        }

        return nil
    }

    /// Returns `true` if the target is destructive (backward-compatible).
    /// Internally delegates to `firstViolation(…)`.
    public func isDestructive(role: String?, title: String?, label: String?, value: String?) -> Bool {
        firstViolation(role: role, title: title, label: label, value: value) == .destructiveAction
    }

    public mutating func addDenyKeyword(_ keyword: String) {
        extraDenyKeywords.insert(keyword.lowercased())
    }

    public mutating func allowKeyword(_ keyword: String) {
        allowedKeywords.insert(keyword.lowercased())
    }

}

private extension PolicyScope {
    func matches(_ requested: PolicyScope) -> Bool {
        (agent == nil || agent == requested.agent)
            && (application == nil || application == requested.application)
            && (workflow == nil || workflow == requested.workflow)
    }
}

// MARK: - Flag name mapping

extension PermissionFlags {
    /// Canonical human-readable names of the grantable flags, in bit order.
    public static let allFlagNames: [String] = [
        "capture", "input", "app_control", "menu_action", "policy_modify",
    ]

    /// Names that were once advertised as grantable but never gated anything.
    /// They are still recognised so an existing `--grant` value or
    /// `~/.config/symoperate/policy.json` keeps starting the server; they grant
    /// nothing, which is exactly what they did before.
    public static let retiredGrantNames: Set<String> = [
        "destructive_action", "secure_field_access",
    ]

    /// Returns the flag for a human-readable name, or `nil` for unknown and
    /// retired names.
    public static func flag(named name: String) -> PermissionFlags? {
        switch name.lowercased() {
        case "capture": return .capture
        case "input": return .input
        case "app_control": return .appControl
        case "menu_action": return .menuAction
        case "policy_modify": return .policyModify
        default: return nil
        }
    }

    /// Parses an array of flag names into a set. Unknown names are ignored.
    public static func parse(names: [String]) -> PermissionFlags {
        var result = PermissionFlags()
        for name in names {
            if let flag = flag(named: name) {
                result.insert(flag)
            }
        }
        return result
    }

    /// Human-readable names of the flags present in this set, in canonical order.
    public var flagNames: [String] {
        Self.allFlagNames.filter { name in
            guard let flag = Self.flag(named: name) else { return false }
            return contains(flag)
        }
    }
}
