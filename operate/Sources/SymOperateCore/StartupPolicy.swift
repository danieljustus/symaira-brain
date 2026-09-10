import Foundation

/// Resolves the permission ceiling applied when an operate process starts.
///
/// A `--grant` value is authoritative when present. Otherwise the optional
/// `~/.config/symoperate/policy.json` file is read. With neither source, the
/// historical full-grant behavior is retained.
public enum StartupPolicy {
    public static var defaultPolicyURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config", isDirectory: true)
            .appendingPathComponent("symoperate", isDirectory: true)
            .appendingPathComponent("policy.json")
    }

    /// Loads a startup policy. `grantNames == nil` means no flag was supplied;
    /// an empty array means `--grant` was supplied with no permissions.
    public static func load(grantNames: [String]?, policyURL: URL? = nil) throws -> ActionPolicy {
        if let grantNames {
            let permissions = try parseGrantNames(grantNames)
            return ActionPolicy(grantedPermissions: permissions, startupGrantedPermissions: permissions)
        }

        let url = policyURL ?? defaultPolicyURL
        guard FileManager.default.fileExists(atPath: url.path) else {
            return ActionPolicy()
        }
        do {
            let data = try Data(contentsOf: url)
            let json = try JSONSerialization.jsonObject(with: data)
            guard let names = permissionNames(in: json) else {
                throw AutomationError.invalidArgument("Startup policy must contain a 'grant' or 'granted_permissions' array.")
            }
            let permissions = try parseGrantNames(names)
            let scopedGrants = try parseScopedGrants(json)
            return ActionPolicy(
                grantedPermissions: permissions,
                startupGrantedPermissions: permissions,
                scopedGrants: scopedGrants
            )
        } catch let error as AutomationError {
            throw error
        } catch {
            throw AutomationError.invalidArgument("Unable to read startup policy at \(url.path): \(error.localizedDescription)")
        }
    }

    /// Parses canonical permission names, rejecting typos rather than silently
    /// starting with a broader grant than the operator requested.
    ///
    /// Names in ``PermissionFlags/retiredGrantNames`` are the one exception:
    /// they were advertised as grantable while gating nothing, so an existing
    /// `--grant` value or policy file may still list them. Refusing those would
    /// stop the server from starting over a permission that never had an
    /// effect, so they are accepted and contribute nothing to the grant.
    public static func parseGrantNames(_ names: [String]) throws -> PermissionFlags {
        var permissions = PermissionFlags()
        for name in names {
            let normalized = name.trimmingCharacters(in: .whitespacesAndNewlines)
            if PermissionFlags.retiredGrantNames.contains(normalized.lowercased()) { continue }
            guard !normalized.isEmpty, let flag = PermissionFlags.flag(named: normalized) else {
                throw AutomationError.invalidArgument("Unknown grant permission '\(name)'.")
            }
            permissions.insert(flag)
        }
        return permissions
    }

    /// Parses scoped grants from a policy JSON object. The preferred shape is:
    /// `scoped_grants: [{agent, application, workflow, granted_permissions}]`.
    /// The compact `agents`/`applications`/`workflows` maps are also accepted.
    public static func parseScopedGrants(_ json: Any) throws -> [ScopedPermissionGrant] {
        guard let object = json as? [String: Any] else { return [] }
        let raw = object["scoped_grants"] ?? object["scopedGrants"]
        var result: [ScopedPermissionGrant] = []
        if let records = raw as? [[String: Any]] {
            for record in records {
                result.append(try parseScopedGrant(record))
            }
        } else if let map = raw as? [String: Any] {
            try appendCompactGrants(from: map, to: &result)
        }
        for key in ["scopes", "scope_grants", "scopeGrants"] {
            if let map = object[key] as? [String: Any] {
                try appendCompactGrants(from: map, to: &result)
            }
        }
        return result
    }

    private static func appendCompactGrants(
        from map: [String: Any],
        to result: inout [ScopedPermissionGrant]
    ) throws {
        for (dimension, rawEntries) in map {
            guard let entries = rawEntries as? [String: Any] else { continue }
            for (identity, rawPermissions) in entries {
                guard let names = rawPermissions as? [String] else {
                    throw AutomationError.invalidArgument("Scoped grant '\(identity)' must contain a permission array.")
                }
                let scope: PolicyScope
                switch dimension.lowercased() {
                case "agents", "agent_grants", "agent":
                    scope = PolicyScope(agent: identity)
                case "applications", "application_grants", "application", "apps":
                    scope = PolicyScope(application: identity)
                case "workflows", "workflow_grants", "workflow":
                    scope = PolicyScope(workflow: identity)
                default:
                    continue
                }
                result.append(ScopedPermissionGrant(scope: scope, permissions: try parseGrantNames(names)))
            }
        }
    }

    private static func parseScopedGrant(_ record: [String: Any]) throws -> ScopedPermissionGrant {
        func stringValue(_ keys: [String]) throws -> String? {
            guard let key = keys.first(where: { record[$0] != nil }) else { return nil }
            guard let value = record[key] as? String, !value.isEmpty else {
                throw AutomationError.invalidArgument("Scoped grant '\(key)' must be a non-empty string.")
            }
            return value
        }
        let scope = PolicyScope(
            agent: try stringValue(["agent", "agent_id"]),
            application: try stringValue(["application", "application_id", "bundle_id"]),
            workflow: try stringValue(["workflow", "workflow_id"])
        )
        guard !scope.isEmpty else {
            throw AutomationError.invalidArgument("Scoped grants must identify an agent, application, or workflow.")
        }
        guard let rawPermissions = record["granted_permissions"] ?? record["grantedPermissions"] ?? record["permissions"] ?? record["grant"] else {
            throw AutomationError.invalidArgument("Scoped grants must contain a 'granted_permissions' array.")
        }
        guard let names = rawPermissions as? [String] else {
            throw AutomationError.invalidArgument("Scoped grant permissions must be an array of strings.")
        }
        return ScopedPermissionGrant(scope: scope, permissions: try parseGrantNames(names))
    }

    private static func permissionNames(in json: Any) -> [String]? {
        if let names = json as? [String] { return names }
        guard let object = json as? [String: Any] else { return nil }
        for key in ["grant", "grants", "granted_permissions", "grantedPermissions"] {
            if let names = object[key] as? [String] { return names }
        }
        if let nested = object["policy"] {
            return permissionNames(in: nested)
        }
        return nil
    }
}
