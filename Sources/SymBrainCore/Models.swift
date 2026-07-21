// Models — Decodable types matching the exact JSON output of `symbrain --json`.

import Foundation

// MARK: - version --json

/// Result of `symbrain version --json`.
public struct VersionInfo: Decodable, Sendable, Equatable {
    public let tool: String
    public let version: String
    public let schemaVersion: Int
    public let goVersion: String?
    public let os: String?
    public let arch: String?
}

// MARK: - doctor --json

/// Full result of `symbrain doctor --json`.
public struct DoctorReport: Decodable, Sendable {
    public let configDir: DirStatus
    public let dataDir: DirStatus
    public let cacheDir: DirStatus
    public let config: ConfigStatus
    public let servers: [ServerStatus]
    public let profiles: [String]
    public let harnesses: [HarnessStatus]
}

public struct DirStatus: Decodable, Sendable {
    public let path: String
    public let exists: Bool
}

public struct ConfigStatus: Decodable, Sendable {
    public let path: String
    public let exists: Bool
    public let parsed: Bool
    public let error: String?
}

public struct ServerStatus: Decodable, Sendable {
    public let name: String
    public let binary: String
    public let found: Bool
    public let path: String?
    public let version: String?
    public let probeError: String?
    public let installHint: String?
}

public struct HarnessStatus: Decodable, Sendable {
    public let name: String
    public let configPath: String
    public let configFound: Bool
    public let configParsed: Bool
    public let installed: Bool
    public let profile: String?
    public let profileExists: Bool
    public let profileMissing: Bool
}

// MARK: - profile list --json

/// One entry from `symbrain profile list --json`.
public struct ProfileSummary: Decodable, Sendable, Identifiable {
    public let name: String
    public let description: String
    public let servers: [ProfileServerRef]

    public var id: String { name }
}

public struct ProfileServerRef: Decodable, Sendable {
    public let server: String
    public let enabled: Bool
    public let mode: String?
}

// MARK: - profile show <name> --json

/// Full result of `symbrain profile show <name> --json`.
public struct ProfileDetail: Decodable, Sendable {
    public let name: String
    public let description: String
    public let audit: AuditStatus?
    public let warnings: [String]?
    public let servers: [ProfileServerDetail]
}

public struct AuditStatus: Decodable, Sendable {
    public let enabled: Bool
}

public struct ProfileServerDetail: Decodable, Sendable {
    public let server: String
    public let enabled: Bool
    public let mode: String?
    public let toolsAllow: [String]?
    public let toolsDeny: [String]?
    public let note: String?
    public let effectivePolicy: EffectivePolicy?
}

public struct EffectivePolicy: Decodable, Sendable {
    public let server: String
    public let enabled: Bool
    public let mode: String?
    public let exposed: [String]
    public let hidden: [String]
    public let unknown: [String]
}

// MARK: - Audit log entry

/// One JSONL audit record, matching the Go `audit.Entry` shape.
public struct AuditEntry: Decodable, Sendable, Identifiable {
    public let timestamp: String
    public let profile: String
    public let server: String
    public let tool: String
    public let durationMs: Int64
    public let status: String
    public let argKeys: String?
    public let argValues: String?

    public var id: String { "\(timestamp)-\(tool)-\(UUID())" }

    /// Parsed date from the RFC3339Nano timestamp.
    public var parsedDate: Date? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: timestamp)
    }

    /// Formatted local time string.
    public var formattedTime: String {
        guard let date = parsedDate else { return timestamp }
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        formatter.timeZone = .current
        return formatter.string(from: date)
    }
}
