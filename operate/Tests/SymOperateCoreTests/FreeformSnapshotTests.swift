import XCTest
import SQLite3
import CryptoKit
import FreeformSnapshotCore
import SymOperateMCP
@testable import SymOperateCore

private final class SnapshotRunner: FreeformSnapshotRunner {
    var calls: [[String]] = []
    var status: Int32 = 0
    var timedOut = false
    var action: (() throws -> Void)?
    func kickstart(arguments: [String], timeout: TimeInterval) throws -> (status: Int32, timedOut: Bool) {
        calls.append(arguments)
        try action?()
        return (status, timedOut)
    }
}

final class FreeformSnapshotTests: XCTestCase {
    private var root: URL!
    override func setUpWithError() throws {
        root = operateTestTemporaryDirectory().appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }
    override func tearDownWithError() throws { try FileManager.default.removeItem(at: root) }

    private func execute(_ db: OpaquePointer?, _ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else {
            throw AutomationError.unavailable(String(cString: sqlite3_errmsg(db)))
        }
    }
    private func scalar(_ db: OpaquePointer?, _ sql: String) throws -> String {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw AutomationError.unavailable("prepare")
        }
        defer { sqlite3_finalize(statement) }
        XCTAssertEqual(sqlite3_step(statement), SQLITE_ROW)
        return String(cString: try XCTUnwrap(sqlite3_column_text(statement, 0)))
    }
    private func digest(_ url: URL) throws -> String {
        SHA256.hash(data: try Data(contentsOf: url)).map { String(format: "%02x", $0) }.joined()
    }
    private func mode(_ url: URL) throws -> Int {
        try XCTUnwrap(FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber).intValue
    }

    func testWALBackupIncludesUncheckpointedRowsIsPrivateAtomicAndLeavesSourceUnchanged() throws {
        let source = root.appendingPathComponent("source with # and ?.db")
        let destination = root.appendingPathComponent("snapshot")
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(source.path, &db), SQLITE_OK)
        defer { sqlite3_close(db) }
        try execute(db, "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE evidence (value TEXT); INSERT INTO evidence VALUES ('fresh WAL');")
        let mainBefore = try digest(source)
        let wal = URL(fileURLWithPath: source.path + "-wal")
        let walBefore = try digest(wal)
        let result = try FreeformSnapshot.run(source: source, destination: destination)
        XCTAssertTrue(result.ok, result.error ?? "")
        XCTAssertEqual(result.helper_version, "1")
        XCTAssertNotNil(result.source_mtime)
        XCTAssertNotNil(result.source_size)
        XCTAssertGreaterThan(result.bytes ?? 0, 0)
        let published = destination.appendingPathComponent("boards.db")
        XCTAssertEqual(try mode(destination), 0o700)
        XCTAssertEqual(try mode(published), 0o600)
        XCTAssertEqual(try mode(destination.appendingPathComponent("status.json")), 0o600)
        var copy: OpaquePointer?
        XCTAssertEqual(sqlite3_open_v2(published.path, &copy, SQLITE_OPEN_READONLY, nil), SQLITE_OK)
        defer { sqlite3_close(copy) }
        XCTAssertEqual(try scalar(copy, "PRAGMA journal_mode"), "delete")
        XCTAssertEqual(try scalar(copy, "PRAGMA quick_check"), "ok")
        XCTAssertEqual(try scalar(copy, "SELECT value FROM evidence"), "fresh WAL")
        XCTAssertEqual(try digest(wal), walBefore)
        // A held file descriptor retains the complete old inode across atomic publication.
        // SQLite itself does not support continuing a connection after its pathname is replaced.
        sqlite3_close(copy)
        copy = nil
        let oldBytes = try Data(contentsOf: published)
        let oldFile = try FileHandle(forReadingFrom: published)
        defer { try? oldFile.close() }
        try execute(db, "INSERT INTO evidence VALUES ('second generation');")
        XCTAssertTrue(try FreeformSnapshot.run(source: source, destination: destination).ok)
        XCTAssertEqual(try oldFile.readToEnd(), oldBytes)
        var next: OpaquePointer?
        XCTAssertEqual(sqlite3_open_v2(published.path, &next, SQLITE_OPEN_READONLY, nil), SQLITE_OK)
        defer { sqlite3_close(next) }
        XCTAssertEqual(try scalar(next, "SELECT count(*) FROM evidence"), "2")
        XCTAssertEqual(try digest(source), mainBefore)
        // First run did not modify source WAL. The second row intentionally did, so check in a separate run.
        XCTAssertNotEqual(try digest(wal), walBefore)
        let beforeThird = try digest(wal)
        XCTAssertTrue(try FreeformSnapshot.run(source: source, destination: destination).ok)
        XCTAssertEqual(try digest(wal), beforeThird)
        XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: destination.path)), ["boards.db", "status.json"])
    }

    func testFailureWritesStatusAndPreservesLastSnapshot() throws {
        let destination = root.appendingPathComponent("snapshot")
        try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
        let previous = destination.appendingPathComponent("boards.db")
        try Data("previous".utf8).write(to: previous)
        let status = try FreeformSnapshot.run(source: root.appendingPathComponent("missing.db"), destination: destination)
        XCTAssertFalse(status.ok)
        XCTAssertTrue(status.error?.contains("Full Disk Access") == true)
        let readback = try JSONDecoder().decode(FreeformHelperStatus.self, from: Data(contentsOf: destination.appendingPathComponent("status.json")))
        XCTAssertFalse(readback.ok)
        XCTAssertEqual(try String(contentsOf: previous, encoding: .utf8), "previous")
        XCTAssertEqual(try mode(destination.appendingPathComponent("status.json")), 0o600)
        XCTAssertEqual(Set(try FileManager.default.contentsOfDirectory(atPath: destination.path)), ["boards.db", "status.json"])
    }

    private func service(_ runner: SnapshotRunner, installed: Bool = true) throws -> FreeformSnapshotService {
        let plist = root.appendingPathComponent("helper.plist")
        if installed { try Data().write(to: plist) }
        return FreeformSnapshotService(directory: root, plist: plist, runner: runner, uid: 501)
    }
    private func publish(ok: Bool, at: Double = Date().timeIntervalSince1970, error: String? = nil) throws {
        let status = FreeformHelperStatus(ok: ok, created_at: at, error: error)
        try JSONEncoder().encode(status).write(to: root.appendingPathComponent("status.json"), options: .atomic)
        if ok { try Data().write(to: root.appendingPathComponent("boards.db")) }
    }

    func testRunnerStartsJobBeforeAcceptingFreshStatus() throws {
        let runner = SnapshotRunner()
        let service = try service(runner)
        try publish(ok: true, at: 1)
        runner.action = { try self.publish(ok: true) }
        let result = service.snapshot(timeout: 0.2)
        XCTAssertTrue(result.ok)
        XCTAssertEqual(result.snapshot_path, root.appendingPathComponent("boards.db").path)
        XCTAssertEqual(runner.calls, [["kickstart", "-p", "gui/501/com.symaira.operate.freeform-snapshot"]])
        XCTAssertNotNil(result.created_at)
    }

    func testStaleAndMalformedStatusTimeOut() throws {
        let runner = SnapshotRunner()
        let service = try service(runner)
        try publish(ok: true, at: 1)
        for malformed in [false, true] {
            if malformed { try Data("not json".utf8).write(to: root.appendingPathComponent("status.json")) }
            let start = ProcessInfo.processInfo.systemUptime
            let result = service.snapshot(timeout: 0.05)
            XCTAssertFalse(result.ok)
            XCTAssertTrue(result.error?.contains("timed out") == true)
            XCTAssertLessThan(ProcessInfo.processInfo.systemUptime - start, 1)
        }
    }

    func testUnregisteredMissingAndRunnerTimeoutAreDistinct() throws {
        let runner = SnapshotRunner()
        let absent = try service(runner, installed: false)
        XCTAssertTrue(absent.snapshot(timeout: 0.1).error?.contains("bootstrap gui/501") == true)
        XCTAssertTrue(runner.calls.isEmpty)
        let installed = try service(runner)
        runner.status = 113
        XCTAssertTrue(installed.snapshot(timeout: 0.1).error?.contains("launchctl exit 113") == true)
        runner.timedOut = true
        XCTAssertTrue(installed.snapshot(timeout: 0.1).error?.contains("timed out while starting") == true)
    }

    func testFDAFailureAndDoctorReadsDoNotKickstart() throws {
        let runner = SnapshotRunner()
        let service = try service(runner)
        try publish(ok: false, error: "FDA missing")
        XCTAssertTrue(service.helperInstalled)
        XCTAssertEqual(service.lastStatus?.error, "FDA missing")
        XCTAssertTrue(runner.calls.isEmpty)
        runner.action = { try self.publish(ok: false, error: "FDA missing") }
        let result = service.snapshot(timeout: 0.1)
        XCTAssertFalse(result.ok)
        XCTAssertEqual(result.error, "FDA missing")
        XCTAssertNil(result.snapshot_path)
    }

    func testInvalidTimeoutDoesNotStartJob() throws {
        let runner = SnapshotRunner()
        let service = try service(runner)
        for timeout in [0, -1, .infinity, .nan, 61] { XCTAssertFalse(service.snapshot(timeout: timeout).ok) }
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testListingMetadataIncludesEmptyBoardsAndDoesNotInferTitles() throws {
        let path = root.appendingPathComponent("boards.db").path
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(path, &db), SQLITE_OK)
        defer { sqlite3_close(db) }
        let a = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        let b = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
        try execute(db, """
            CREATE TABLE boards (board_identifier BLOB, last_activity_time REAL, data BLOB);
            CREATE TABLE board_items (board_identifier BLOB, tombstoned INT, specific_data BLOB);
            INSERT INTO boards VALUES (X'\(a)',123.5,X'63726474060000000A0B5365637265742074657874');
            INSERT INTO boards VALUES (X'\(b)',NULL,NULL);
            INSERT INTO board_items VALUES (X'\(a)',0,X'FF');
            INSERT INTO board_items VALUES (X'\(a)',1,X'FF');
            """)
        let boards = try FreeformBoardReader().listBoards(snapshotPath: path)
        XCTAssertEqual(boards.count, 2)
        XCTAssertEqual(boards[0].id, UUID(uuidString: "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"))
        XCTAssertEqual(boards[0].item_count, 1)
        XCTAssertEqual(boards[0].last_activity_time, 123.5)
        XCTAssertEqual(boards[1].item_count, 0)
        XCTAssertNil(boards[1].last_activity_time)
        let json = try String(decoding: JSONEncoder().encode(boards), as: UTF8.self)
        XCTAssertFalse(json.contains("Secret"))
        XCTAssertFalse(json.contains("title"))
        try execute(db, "ALTER TABLE boards DROP COLUMN last_activity_time;")
        XCTAssertNil(try FreeformBoardReader().listBoards(snapshotPath: path)[0].last_activity_time)
        XCTAssertThrowsError(try FreeformBoardReader().read(snapshotPath: path))
    }

    /// Field 6 string table as observed on Freeform 5.1: "a", "b", title, "c", "d", ...
    private func boardData(_ strings: [String]) -> Data {
        var table: [UInt8] = []
        for string in strings { table += [0x12, UInt8(string.utf8.count)] + Array(string.utf8) }
        return Data([0x63, 0x72, 0x64, 0x74, 6, 0, 0, 0, 0x08, 0x01, 0x32, UInt8(table.count)] + table)
    }

    func testBoardTitleIsDecodedOnlyFromTheObservedStringTableLayout() throws {
        XCTAssertEqual(try FreeformCRDT.boardTitle(boardData(["a", "b", "Fußbodenheizung", "c", "d", "e"])), "Fußbodenheizung")
        XCTAssertNil(try FreeformCRDT.boardTitle(boardData(["a", "Secret", "c", "d", "e"])))
        XCTAssertNil(try FreeformCRDT.boardTitle(boardData(["x", "b", "Title", "c", "d"])))
        XCTAssertNil(try FreeformCRDT.boardTitle(Data()))
        XCTAssertThrowsError(try FreeformCRDT.boardTitle(Data("notcrdt!".utf8)))
    }
}
