import XCTest
import SQLite3
@testable import SymOperateCore

final class FreeformBoardReaderTests: XCTestCase {
    private let header: [UInt8] = [0x63, 0x72, 0x64, 0x74, 6, 0, 0, 0]

    private func crdt(_ bytes: [UInt8]) -> Data { Data(header + bytes) }
    private func stringField(_ text: String) -> [UInt8] {
        let bytes = Array(text.utf8)
        precondition(bytes.count < 128)
        return [0x0A, UInt8(bytes.count)] + bytes
    }
    private func pair(_ first: Float, _ second: Float) -> [UInt8] {
        func little(_ value: Float) -> [UInt8] { (0..<4).map { UInt8(truncatingIfNeeded: value.bitPattern >> ($0 * 8)) } }
        return [0x22, 0x0A, 0x0D] + little(first) + [0x15] + little(second)
    }

    func testVarintsKnownValuesOverflowAndTruncation() throws {
        var offset = 0
        XCTAssertEqual(try FreeformCRDT.readVarint([0xAC, 0x02], offset: &offset), 300)
        XCTAssertEqual(offset, 2)
        for bytes: [UInt8] in [[0x80], Array(repeating: 0xFF, count: 10), Array(repeating: 0x80, count: 10)] {
            offset = 0
            XCTAssertThrowsError(try FreeformCRDT.readVarint(bytes, offset: &offset))
        }
    }

    func testGeometryAndTextFromSyntheticCRDT() throws {
        XCTAssertEqual(try FreeformCRDT.geometry(crdt(pair(10.5, -20) + pair(140, 70))), RectValue(x: 10.5, y: -20, width: 140, height: 70))
        let strings = stringField("fontname") + stringField("Hello board\nattribute garbage") + stringField("fontsize")
        XCTAssertEqual(try FreeformCRDT.text(crdt(strings)), "Hello board")
    }

    func testNestedTextAndUnicode() throws {
        let inner = stringField("東京")
        XCTAssertEqual(try FreeformCRDT.text(crdt([0x12, UInt8(inner.count)] + inner)), "東京")
    }

    func testInvalidHeadersTruncationAndWireTypesFailClosed() {
        for data in [Data("crdt".utf8), Data([0x63, 0x72, 0x64, 0x74, 7, 0, 0, 0]),
                     crdt([0x0A, 100, 65]), crdt([0x0F]), crdt([0x09, 1]), crdt([0])] {
            XCTAssertThrowsError(try FreeformCRDT.text(data))
        }
    }

    func testInvalidGeometryDoesNotProduceCoordinates() throws {
        XCTAssertNil(try FreeformCRDT.geometry(crdt(pair(.infinity, 1) + pair(20, 20))))
        XCTAssertNil(try FreeformCRDT.geometry(crdt(pair(1, 1) + pair(-1, 20))))
        XCTAssertNil(try FreeformCRDT.geometry(crdt(pair(1, 1))))
        XCTAssertEqual(try FreeformCRDT.text(Data()), "")
    }

    func testSnapshotReadFilterTombstonesTextGeometryAndLineEndpointEvidence() throws {
        let directory = operateTestTemporaryDirectory().appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = directory.appendingPathComponent("synthetic.sqlite").path
        var database: OpaquePointer?
        XCTAssertEqual(sqlite3_open(path, &database), SQLITE_OK)
        let db = try XCTUnwrap(database)
        defer { sqlite3_close(db) }
        XCTAssertEqual(sqlite3_exec(db, "CREATE TABLE board_items (item_uuid BLOB, parent_uuid BLOB, board_identifier BLOB, item_type INT, common_data BLOB, specific_data BLOB, tombstoned INT); CREATE TABLE boards (board_identifier BLOB, data BLOB);", nil, nil, nil), SQLITE_OK)
        let board = UUID(uuidString: "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA")!
        let otherBoard = UUID(uuidString: "BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB")!
        let first = UUID(uuidString: "11111111-1111-1111-1111-111111111111")!
        let second = UUID(uuidString: "22222222-2222-2222-2222-222222222222")!
        let line = UUID(uuidString: "33333333-3333-3333-3333-333333333333")!
        func uuidData(_ id: UUID) -> Data { var bytes = id.uuid; return withUnsafeBytes(of: &bytes) { Data($0) } }
        func hex(_ data: Data) -> String { data.map { String(format: "%02x", $0) }.joined() }
        func insert(_ id: UUID, _ boardID: UUID, type: Int, tombstone: Int = 0, specific: Data) {
            let common = crdt(pair(10, 20) + pair(140, 70))
            let sql = "INSERT INTO board_items VALUES (X'\(hex(uuidData(id)))', NULL, X'\(hex(uuidData(boardID)))', \(type), X'\(hex(common))', X'\(hex(specific))', \(tombstone));"
            XCTAssertEqual(sqlite3_exec(db, sql, nil, nil, nil), SQLITE_OK)
        }
        insert(first, board, type: 3, specific: crdt(stringField("Alpha\nfont garbage")))
        insert(second, board, type: 12, specific: crdt(stringField("Beta")))
        insert(line, board, type: 4, specific: crdt([0x0A, 16] + uuidData(first) + [0x12, 16] + uuidData(second)))
        insert(UUID(), board, type: 3, tombstone: 1, specific: crdt(stringField("Deleted")))
        insert(UUID(), otherBoard, type: 3, specific: crdt(stringField("Other board")))
        let before = try Data(contentsOf: URL(fileURLWithPath: path))
        let reader = FreeformBoardReader()
        let items = try reader.read(snapshotPath: path, boardIdentifier: board)
        XCTAssertEqual(items.count, 3)
        XCTAssertEqual(items.first { $0.id == first }?.text, "Alpha")
        XCTAssertEqual(items.first { $0.id == first }?.geometry, RectValue(x: 10, y: 20, width: 140, height: 70))
        XCTAssertEqual(Set(items.first { $0.id == line }!.endpointIDs), [first, second])
        XCTAssertThrowsError(try reader.read(snapshotPath: path), "Unscoped object contents must fail closed")
        XCTAssertEqual(try reader.read(snapshotPath: path, boardIdentifier: UUID()).count, 0)
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: path)), before, "Read-only reader must leave the file byte-identical")
    }

    func testReaderRejectsRelativeMissingAndLiveStoragePaths() {
        let reader = FreeformBoardReader()
        for path in ["relative.sqlite", "/nonexistent/snapshot.sqlite", "/Users/example/Library/Group Containers/group.com.apple.freeform/Boards/boards.db"] {
            XCTAssertThrowsError(try reader.read(snapshotPath: path))
        }
    }
}
