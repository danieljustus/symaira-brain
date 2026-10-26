import Foundation
import SQLite3

public struct FreeformBoardItem: Codable, Sendable {
    public let id: UUID
    public let boardIdentifier: UUID
    public let itemType: Int
    public let geometry: RectValue?
    public let text: String
    /// UUID occurrence evidence only; head/tail ordering is not claimed.
    public let endpointIDs: [UUID]

    enum CodingKeys: String, CodingKey {
        case id, geometry, text
        case boardIdentifier = "board_identifier", itemType = "item_type", endpointIDs = "endpoint_ids"
    }
}

/// Phase 1 reads an explicitly supplied, pre-existing SQLite snapshot only.
/// No backup creation, live database lookup, launchd, TCC request or helper process.
public struct FreeformBoardReader: Sendable {
    private struct Row {
        let id: UUID
        let board: UUID
        let type: Int
        let common: Data
        let specific: Data
    }

    public init() {}

    public func read(snapshotPath: String, boardIdentifier: UUID? = nil) throws -> [FreeformBoardItem] {
        guard boardIdentifier != nil else {
            throw AutomationError.invalidArgument("board_identifier is required for object contents. Use freeform_list_boards first.")
        }
        guard snapshotPath.hasPrefix("/") else { throw AutomationError.invalidArgument("snapshot_path must be absolute.") }
        let url = URL(fileURLWithPath: snapshotPath).standardizedFileURL.resolvingSymlinksInPath()
        // Refuse the known live storage tree, also through a symlink. The caller owns snapshot provenance.
        guard !url.path.contains("/Library/Group Containers/group.com.apple.freeform/"),
              FileManager.default.fileExists(atPath: url.path) else {
            throw AutomationError.invalidArgument("Supply an existing snapshot outside Freeform live storage.")
        }
        var database: OpaquePointer?
        let opened = sqlite3_open_v2(url.path, &database, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil)
        guard opened == SQLITE_OK, let database else {
            if let database { sqlite3_close(database) }
            throw AutomationError.unavailable("Cannot open SQLite snapshot read-only.")
        }
        defer { sqlite3_close(database) }
        sqlite3_busy_timeout(database, 500)
        sqlite3_limit(database, SQLITE_LIMIT_LENGTH, 16 * 1024 * 1024)
        // A transaction prevents inconsistent rows even if a caller replaces a non-immutable snapshot.
        guard sqlite3_exec(database, "BEGIN", nil, nil, nil) == SQLITE_OK else {
            throw AutomationError.unavailable("Cannot begin snapshot read transaction.")
        }
        defer { sqlite3_exec(database, "ROLLBACK", nil, nil, nil) }
        let sql = """
            SELECT item_uuid, board_identifier, item_type, common_data, specific_data
            FROM board_items WHERE tombstoned = 0
            \(boardIdentifier == nil ? "" : "AND board_identifier = ?")
            ORDER BY board_identifier, item_uuid LIMIT 10001
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            // Surface SQLite's reason: a WAL-mode snapshot without its -shm sidecar fails here too
            // (observed live) and is not a schema problem.
            let reason = String(cString: sqlite3_errmsg(database))
            throw AutomationError.invalidArgument(
                "Cannot query board_items in snapshot (\(reason)). Expected the observed Freeform schema in a non-WAL snapshot.")
        }
        defer { sqlite3_finalize(statement) }
        if let boardIdentifier {
            var uuid = boardIdentifier.uuid
            let result = withUnsafeBytes(of: &uuid) { bytes in
                sqlite3_bind_blob(statement, 1, bytes.baseAddress, 16, unsafeBitCast(-1, to: sqlite3_destructor_type.self))
            }
            guard result == SQLITE_OK else { throw AutomationError.unavailable("Cannot bind board identifier.") }
        }
        var rows: [Row] = []
        var totalBlobBytes = 0
        var status = sqlite3_step(statement)
        while status == SQLITE_ROW {
            guard rows.count < 10000, let id = Self.uuid(blob(statement, column: 0)),
                  let board = Self.uuid(blob(statement, column: 1)) else {
                throw AutomationError.invalidArgument("Snapshot has invalid UUIDs or exceeds 10000 items.")
            }
            totalBlobBytes += Int(sqlite3_column_bytes(statement, 3)) + Int(sqlite3_column_bytes(statement, 4))
            guard totalBlobBytes <= 64 * 1024 * 1024 else {
                throw AutomationError.invalidArgument("Snapshot selection exceeds the 64 MiB blob limit.")
            }
            rows.append(Row(id: id, board: board, type: Int(sqlite3_column_int(statement, 2)),
                            common: blob(statement, column: 3), specific: blob(statement, column: 4)))
            status = sqlite3_step(statement)
        }
        guard status == SQLITE_DONE else { throw AutomationError.unavailable("SQLite snapshot read failed.") }
        return try rows.map { row in
            let endpoints = row.type == 4 ? rows.filter { other in
                guard other.board == row.board, other.id != row.id, other.type != 4 else { return false }
                var uuid = other.id.uuid
                let bytes = withUnsafeBytes(of: &uuid) { Data($0) }
                return row.common.range(of: bytes) != nil || row.specific.range(of: bytes) != nil
            }.map(\.id) : []
            return FreeformBoardItem(id: row.id, boardIdentifier: row.board, itemType: row.type,
                                     geometry: try FreeformCRDT.geometry(row.common),
                                     text: row.type == 4 ? "" : try FreeformCRDT.text(row.specific), endpointIDs: endpoints)
        }
    }

    public struct BoardSummary: Codable, Sendable {
        public let id: UUID
        /// Stored SQLite value, without guessing an epoch or unit.
        public let last_activity_time: Double?
        public let item_count: Int
    }

    /// Metadata only. Never select object blobs or infer a title from an arbitrary string in boards.data.
    public func listBoards(snapshotPath: String) throws -> [BoardSummary] {
        guard snapshotPath.hasPrefix("/") else { throw AutomationError.invalidArgument("snapshot_path must be absolute.") }
        let url = URL(fileURLWithPath: snapshotPath).standardizedFileURL.resolvingSymlinksInPath()
        guard !url.path.contains("/Library/Group Containers/group.com.apple.freeform/"),
              FileManager.default.fileExists(atPath: url.path) else {
            throw AutomationError.invalidArgument("Supply an existing snapshot outside Freeform live storage.")
        }
        var database: OpaquePointer?
        let opened = sqlite3_open_v2(url.path, &database, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil)
        guard opened == SQLITE_OK, let database else {
            if let database { sqlite3_close(database) }
            throw AutomationError.unavailable("Cannot open SQLite snapshot read-only.")
        }
        defer { sqlite3_close(database) }
        sqlite3_busy_timeout(database, 500)
        sqlite3_limit(database, SQLITE_LIMIT_LENGTH, 16 * 1024 * 1024)
        // Older synthetic/observed variants may omit activity metadata. Never invent a value.
        var probe: OpaquePointer?
        let hasActivity = sqlite3_prepare_v2(database, "SELECT last_activity_time FROM boards LIMIT 0", -1, &probe, nil) == SQLITE_OK
        if let probe { sqlite3_finalize(probe) }
        let sql = """
            SELECT b.board_identifier, \(hasActivity ? "b.last_activity_time" : "NULL"),
              (SELECT count(*) FROM board_items i WHERE i.board_identifier=b.board_identifier AND i.tombstoned=0)
            FROM boards b ORDER BY b.board_identifier LIMIT 10001
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            throw AutomationError.invalidArgument("Cannot query boards metadata in snapshot.")
        }
        defer { sqlite3_finalize(statement) }
        var boards: [BoardSummary] = []
        var status = sqlite3_step(statement)
        while status == SQLITE_ROW {
            guard boards.count < 10000, let id = Self.uuid(blob(statement, column: 0)) else {
                throw AutomationError.invalidArgument("Invalid board identifier or more than 10000 boards.")
            }
            let activity = sqlite3_column_type(statement, 1) == SQLITE_NULL ? nil : sqlite3_column_double(statement, 1)
            guard activity?.isFinite != false else { throw AutomationError.invalidArgument("Invalid board activity time.") }
            boards.append(BoardSummary(id: id, last_activity_time: activity, item_count: Int(sqlite3_column_int64(statement, 2))))
            status = sqlite3_step(statement)
        }
        guard status == SQLITE_DONE else { throw AutomationError.unavailable("SQLite board listing failed.") }
        return boards
    }

    private func blob(_ statement: OpaquePointer, column: Int32) -> Data {
        let size = Int(sqlite3_column_bytes(statement, column))
        guard size > 0, let pointer = sqlite3_column_blob(statement, column) else { return Data() }
        return Data(bytes: pointer, count: size)
    }

    private static func uuid(_ data: Data) -> UUID? {
        let bytes = Array(data)
        guard bytes.count == 16 else { return nil }
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                           bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]))
    }
}
