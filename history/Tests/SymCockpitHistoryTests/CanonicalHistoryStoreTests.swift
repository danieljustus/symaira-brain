import Foundation
import XCTest
@testable import SymCockpitHistory

final class CanonicalHistoryStoreTests: XCTestCase {
    private var fileURL: URL!

    override func setUp() {
        super.setUp()
        fileURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("symcockpit-history-\(UUID().uuidString).jsonl")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: fileURL)
        super.tearDown()
    }

    func testFixtureRoundTripsTuneAndOperateRecords() throws {
        let fixture = Bundle.module.url(forResource: "cross-component", withExtension: "jsonl", subdirectory: "Fixtures")!
        try FileManager.default.copyItem(at: fixture, to: fileURL)

        let records = try CanonicalHistoryStore(fileURL: fileURL).read(limit: nil)
        XCTAssertEqual(records.count, 2)
        XCTAssertEqual(records[0].source, "tune")
        XCTAssertEqual(records[0].action, "brightness.set")
        XCTAssertEqual(records[1].source, "operate")
        XCTAssertEqual(records[1].action, "click")
        XCTAssertEqual(records[1].payload["targets"], .object(["button": .string("left")]))
    }

    func testMalformedAndLegacyLinesAreTolerated() throws {
        let legacy = #"{"action":"dim.set","result":"success","timestamp":"2026-08-28T10:00:02Z"}"#
        let content = "not-json\n{\"schema_version\":999}\n\(legacy)\n{\"action\":\"missing timestamp\"}\n"
        try content.write(to: fileURL, atomically: true, encoding: .utf8)

        let records = try CanonicalHistoryStore(fileURL: fileURL).read(limit: nil)
        XCTAssertEqual(records.count, 1)
        XCTAssertEqual(records[0].source, "legacy")
        XCTAssertEqual(records[0].action, "dim.set")
    }

    func testWriterIsDeterministicVersionedAndRedactsPayload() throws {
        let event = CanonicalHistoryEvent(
            source: "operate",
            timestamp: "2026-08-28T10:00:00Z",
            action: "type_text",
            payload: [
                "message": .string("token=sk-abcdEFGH12345678ijkl"),
                "success": .bool(true),
            ]
        )
        let store = CanonicalHistoryStore(fileURL: fileURL)
        try store.append(event)

        let line = try String(contentsOf: fileURL, encoding: .utf8)
        XCTAssertTrue(line.contains("\"schema_version\":1"))
        XCTAssertTrue(line.contains("<redacted>"))
        XCTAssertFalse(line.contains("sk-abcdEFGH12345678ijkl"))
        XCTAssertEqual(try store.read(limit: nil).first?.payload["message"], .string("token=<redacted>"))
    }
}
