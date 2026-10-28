import Foundation
import SQLite3

/// Minimal one-shot backup core. Only the executable chooses the fixed production source.
public enum FreeformSnapshot {
    public static let version = "1"
    public static let bundleID = "com.symaira.operate.freeform-snapshot"
    public static var directory: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Caches/com.symaira.operate/freeform-snapshot", isDirectory: true)
    }
    public static var source: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Group Containers/group.com.apple.freeform/Boards/boards.db")
    }

    public struct Status: Codable, Sendable {
        public let ok: Bool
        public let created_at: Double
        public let source_mtime: Double?
        public let source_size: UInt64?
        public let bytes: UInt64?
        public let helper_version: String
        public let error: String?

        public init(ok: Bool, created_at: Double, source_mtime: Double? = nil,
                    source_size: UInt64? = nil, bytes: UInt64? = nil, error: String? = nil) {
            self.ok = ok
            self.created_at = created_at
            self.source_mtime = source_mtime
            self.source_size = source_size
            self.bytes = bytes
            self.helper_version = FreeformSnapshot.version
            self.error = error
        }
    }

    private struct Failure: LocalizedError {
        let errorDescription: String?
        init(_ message: String) { errorDescription = message }
    }

    /// Source is opened read-only, including WAL. Publish only a checked, closed DELETE-mode file.
    /// A failed run preserves the previous snapshot but publishes an explicit failure status.
    @discardableResult
    public static func run(source: URL, destination: URL) throws -> Status {
        let manager = FileManager.default
        try manager.createDirectory(at: destination, withIntermediateDirectories: true,
                                    attributes: [.posixPermissions: 0o700])
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: destination.path)
        let temporary = destination.appendingPathComponent(".backup-\(UUID().uuidString).db")
        defer {
            for suffix in ["", "-wal", "-shm", "-journal"] { try? manager.removeItem(atPath: temporary.path + suffix) }
        }
        var metadata: [FileAttributeKey: Any] = [:]
        let status: Status
        do {
            metadata = try manager.attributesOfItem(atPath: source.path)
            guard manager.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
                throw Failure("Cannot create private backup file.")
            }
            try backup(source: source, target: temporary)
            try manager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: temporary.path)
            let size = (try manager.attributesOfItem(atPath: temporary.path)[.size] as? NSNumber)?.uint64Value
            guard rename(temporary.path, destination.appendingPathComponent("boards.db").path) == 0 else {
                throw Failure("Cannot atomically publish snapshot: \(String(cString: strerror(errno)))")
            }
            status = Status(ok: true, created_at: Date().timeIntervalSince1970,
                            source_mtime: (metadata[.modificationDate] as? Date)?.timeIntervalSince1970,
                            source_size: (metadata[.size] as? NSNumber)?.uint64Value, bytes: size)
        } catch {
            status = Status(ok: false, created_at: Date().timeIntervalSince1970,
                            source_mtime: (metadata[.modificationDate] as? Date)?.timeIntervalSince1970,
                            source_size: (metadata[.size] as? NSNumber)?.uint64Value,
                            error: "Freeform backup failed. Check Full Disk Access for Symaira Operate Freeform Snapshot.app. \(error.localizedDescription)")
        }
        let statusTemp = destination.appendingPathComponent(".status-\(UUID().uuidString).json")
        defer { try? manager.removeItem(at: statusTemp) }
        guard manager.createFile(atPath: statusTemp.path, contents: try JSONEncoder().encode(status),
                                 attributes: [.posixPermissions: 0o600]),
              rename(statusTemp.path, destination.appendingPathComponent("status.json").path) == 0 else {
            throw Failure("Cannot atomically publish helper status.")
        }
        return status
    }

    private static func backup(source: URL, target: URL) throws {
        var input: OpaquePointer?
        var output: OpaquePointer?
        defer {
            if let input { sqlite3_close(input) }
            if let output { sqlite3_close(output) }
        }
        // URL.absoluteString percent-encodes spaces, # and ? before adding URI parameters.
        guard sqlite3_open_v2(source.absoluteString + "?mode=ro", &input,
                              SQLITE_OPEN_READONLY | SQLITE_OPEN_URI, nil) == SQLITE_OK,
              let input else { throw Failure("Cannot open Freeform source read-only (missing store or FDA denied).") }
        guard sqlite3_open_v2(target.path, &output, SQLITE_OPEN_READWRITE, nil) == SQLITE_OK,
              let output else { throw Failure("Cannot open backup destination.") }
        sqlite3_busy_timeout(input, 1000)
        guard let backup = sqlite3_backup_init(output, "main", input, "main") else {
            throw Failure("Cannot initialize SQLite backup.")
        }
        let result = sqlite3_backup_step(backup, -1)
        let finished = sqlite3_backup_finish(backup)
        guard result == SQLITE_DONE, finished == SQLITE_OK else {
            throw Failure("SQLite backup failed (\(result), \(finished)).")
        }
        guard try scalar(output, "PRAGMA journal_mode=DELETE").lowercased() == "delete",
              try scalar(output, "PRAGMA quick_check") == "ok" else {
            throw Failure("Backup journal mode or quick_check failed.")
        }
    }

    private static func scalar(_ database: OpaquePointer, _ sql: String) throws -> String {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            throw Failure("Cannot prepare backup validation.")
        }
        defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW, let text = sqlite3_column_text(statement, 0) else {
            throw Failure("Backup validation failed.")
        }
        let value = String(cString: text)
        guard sqlite3_step(statement) == SQLITE_DONE else { throw Failure("Multiple validation results.") }
        return value
    }
}
