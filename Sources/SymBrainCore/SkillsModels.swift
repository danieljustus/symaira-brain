// SkillsModels — Decodable types for the embedded skills core.
//
// These match the JSON emitted by `symbrain skills <command> --output json`.
// As with the Memory and Vault models they use a plain decoder and explicit
// snake_case CodingKeys (see ModuleModels).

import Foundation

// MARK: - list --json

/// Result of `symbrain skills list`.
public struct SkillLibrary: Decodable, Sendable {
    public let skills: [SkillSummary]
    public let categoryCounts: [String: Int]?
    public let issues: [JSONValue]?

    enum CodingKeys: String, CodingKey {
        case skills
        case categoryCounts = "category_counts"
        case issues
    }

    /// Library load problems, rendered for display.
    public var issueMessages: [String] {
        (issues ?? []).map(\.displayString).filter { !$0.isEmpty }
    }
}

/// One library skill from `symbrain skills list`.
public struct SkillSummary: Decodable, Sendable, Identifiable, Equatable {
    public let name: String
    public let description: String?
    public let path: String
    public let createdAt: String?
    public let modifiedAt: String?
    public let lastRenderedAt: String?
    public let installs: [SkillInstallRef]?
    public let lastUsed: String?
    public let lastUsedSource: String?

    enum CodingKeys: String, CodingKey {
        case name
        case description
        case path
        case createdAt = "created_at"
        case modifiedAt = "modified_at"
        case lastRenderedAt = "last_rendered_at"
        case installs
        case lastUsed = "last_used"
        case lastUsedSource = "last_used_source"
    }

    public var id: String { name }

    /// The harnesses this skill is installed into, in a stable order.
    public var targets: [String] {
        (installs ?? []).map(\.target).sorted()
    }

    public var formattedModifiedAt: String? {
        modifiedAt.map(formatModuleTimestamp)
    }

    public var formattedLastUsed: String {
        lastUsed.map(formatModuleTimestamp) ?? "never"
    }
}

/// One install reference inside a `symbrain skills list` entry.
public struct SkillInstallRef: Decodable, Sendable, Equatable {
    public let target: String
    public let path: String?
    public let installedAt: String?

    enum CodingKeys: String, CodingKey {
        case target
        case path
        case installedAt = "installed_at"
    }
}

// MARK: - status --json

/// Result of `symbrain skills status`.
public struct SkillStatusReport: Decodable, Sendable {
    public let installs: [SkillInstallStatus]?
    public let summary: SkillStatusSummary?

    public var rows: [SkillInstallStatus] { installs ?? [] }
}

/// One managed install from `symbrain skills status`.
public struct SkillInstallStatus: Decodable, Sendable, Identifiable, Equatable {
    public let target: String
    public let name: String
    public let path: String?
    public let status: String
    public let mode: String?
    public let installedAt: String?
    public let sourceHash: String?

    enum CodingKeys: String, CodingKey {
        case target
        case name
        case path
        case status
        case mode
        case installedAt = "installed_at"
        case sourceHash = "source_hash"
    }

    public var id: String { "\(target)/\(name)" }

    public var formattedInstalledAt: String? {
        installedAt.map(formatModuleTimestamp)
    }
}

/// The counts block of `symbrain skills status`.
public struct SkillStatusSummary: Decodable, Sendable, Equatable {
    public let inSync: Int?
    public let stale: Int?
    public let harnessChanged: Int?
    public let conflict: Int?
    public let orphaned: Int?
    public let unmanaged: Int?

    enum CodingKeys: String, CodingKey {
        case inSync = "in_sync"
        case stale
        case harnessChanged = "harness_changed"
        case conflict
        case orphaned
        case unmanaged
    }

    /// Non-zero counts as label/value pairs, in reporting order.
    public var badges: [(label: String, count: Int)] {
        [
            ("in sync", inSync ?? 0),
            ("stale", stale ?? 0),
            ("harness changed", harnessChanged ?? 0),
            ("conflict", conflict ?? 0),
            ("orphaned", orphaned ?? 0),
            ("unmanaged", unmanaged ?? 0),
        ]
        .filter { $0.count > 0 }
    }

    /// True when a `symbrain skills sync` would have work to do.
    public var needsSync: Bool {
        (stale ?? 0) > 0 || (harnessChanged ?? 0) > 0
    }
}

// MARK: - targets --json

/// Result of `symbrain skills targets`.
public struct SkillTargetsReport: Decodable, Sendable {
    public let targets: [SkillTarget]?

    public var rows: [SkillTarget] { targets ?? [] }
}

/// One harness from `symbrain skills targets`.
public struct SkillTarget: Decodable, Sendable, Identifiable, Equatable {
    public let target: String
    public let displayName: String?
    public let installed: Bool?
    public let evidence: String?
    public let effectiveSkillRoot: String?
    public let skillRootExists: Bool?
    public let skillRootReadable: Bool?
    public let managedSkillsCount: Int?
    public let unmanagedSkillsCount: Int?
    public let installState: String?
    public let capabilities: [String]?
    public let setupHint: String?
    public let verificationStatus: String?

    enum CodingKeys: String, CodingKey {
        case target
        case displayName = "display_name"
        case installed
        case evidence
        case effectiveSkillRoot = "effective_skill_root"
        case skillRootExists = "skill_root_exists"
        case skillRootReadable = "skill_root_readable"
        case managedSkillsCount = "managed_skills_count"
        case unmanagedSkillsCount = "unmanaged_skills_count"
        case installState = "install_state"
        case capabilities
        case setupHint = "setup_hint"
        case verificationStatus = "verification_status"
    }

    public var id: String { target }

    public var title: String {
        displayName?.isEmpty == false ? displayName! : target
    }
}

// MARK: - log --json

/// One record from `symbrain skills log` — the local operation history.
public struct SkillLogEntry: Decodable, Sendable, Identifiable, Equatable {
    public let ts: String
    public let event: String
    public let skill: String?
    public let target: String?
    public let path: String?
    public let outcome: String?
    public let toolVersion: String?
    public let actor: String?
    public let scope: String?
    public let mode: String?

    enum CodingKeys: String, CodingKey {
        case ts
        case event
        case skill
        case target
        case path
        case outcome
        case toolVersion = "tool_version"
        case actor
        case scope
        case mode
    }

    public var id: String {
        "\(ts)-\(event)-\(skill ?? "")-\(target ?? "")"
    }

    public var formattedTimestamp: String { formatModuleTimestamp(ts) }
}

// MARK: - sync --json

/// Result of `symbrain skills sync [--dry-run]`.
public struct SkillSyncReport: Decodable, Sendable {
    public let results: [SkillSyncResult]?

    public var rows: [SkillSyncResult] { results ?? [] }
}

/// One planned or applied sync action.
public struct SkillSyncResult: Decodable, Sendable, Identifiable, Equatable {
    public let target: String
    public let name: String
    public let path: String?
    public let action: String
    public let mode: String?
    public let error: String?

    public var id: String { "\(target)/\(name)" }
}

// MARK: - doctor --json

/// Result of `symbrain skills doctor`.
public struct SkillsDoctorReport: Decodable, Sendable {
    public let config: SkillsDoctorConfig?
    public let configPath: String?
    public let logPath: String?
    public let profilesDir: String?
    public let projectDir: String?
    public let targets: [SkillsDoctorTarget]?

    enum CodingKeys: String, CodingKey {
        case config
        case configPath = "config_path"
        case logPath = "log_path"
        case profilesDir = "profiles_dir"
        case projectDir = "project_dir"
        case targets
    }

    /// The reported paths as label/value pairs for a detail list.
    public var pathRows: [(label: String, value: String)] {
        var rows: [(String, String)] = []
        if let value = configPath { rows.append(("Config", value)) }
        if let value = config?.libraryDir { rows.append(("Library", value)) }
        if let value = config?.renderDir { rows.append(("Rendered", value)) }
        if let value = config?.cacheDir { rows.append(("Cache", value)) }
        if let value = profilesDir ?? config?.profilesDir { rows.append(("Profiles", value)) }
        if let value = config?.baseDir { rows.append(("Base", value)) }
        if let value = logPath { rows.append(("Log", value)) }
        if let value = projectDir { rows.append(("Project", value)) }
        return rows
    }

    public var versioningEnabled: Bool? { config?.vcs?.enabled }
}

public struct SkillsDoctorConfig: Decodable, Sendable {
    public let libraryDir: String?
    public let renderDir: String?
    public let cacheDir: String?
    public let profilesDir: String?
    public let baseDir: String?
    public let vcs: SkillsDoctorVCS?

    enum CodingKeys: String, CodingKey {
        case libraryDir = "library_dir"
        case renderDir = "render_dir"
        case cacheDir = "cache_dir"
        case profilesDir = "profiles_dir"
        case baseDir = "base_dir"
        case vcs
    }
}

public struct SkillsDoctorVCS: Decodable, Sendable {
    public let enabled: Bool?
}

/// One target path pair from `symbrain skills doctor`.
public struct SkillsDoctorTarget: Decodable, Sendable, Identifiable, Equatable {
    public let target: String
    public let user: String?
    public let project: String?

    public var id: String { target }
}
