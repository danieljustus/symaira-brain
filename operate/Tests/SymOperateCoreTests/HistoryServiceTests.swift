import XCTest
@testable import SymOperateCore

final class HistoryServiceTests: XCTestCase {

    private var tempDir: URL!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("HistoryServiceTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
        tempDir = nil
    }

    private func makeEvent(index: Int) -> HistoryEvent {
        HistoryEvent(action: "click", targets: ["x": "\(index)"], success: true, message: "m\(index)")
    }

    private func permissions(of url: URL) throws -> Int {
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        return attrs[.posixPermissions] as? Int ?? 0
    }

    // MARK: - File permissions

    func testSecureDirectoryApplies0700Permissions() throws {
        let dir = tempDir.appendingPathComponent("share/symoperate")

        HistoryService.secureDirectory(at: dir)

        XCTAssertEqual(try permissions(of: dir), 0o700)

        // Already-existing directory is hardened too.
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
        HistoryService.secureDirectory(at: dir)
        XCTAssertEqual(try permissions(of: dir), 0o700)
    }

    func testInjectedFileIsCreatedWith0600Permissions() throws {
        let fileURL = tempDir.appendingPathComponent("history.jsonl")

        let service = HistoryService(fileURL: fileURL)
        try service.record(makeEvent(index: 0))

        XCTAssertTrue(FileManager.default.fileExists(atPath: fileURL.path))
        XCTAssertEqual(try permissions(of: fileURL), 0o600)
    }

    func testExistingFilePermissionsAreHardenedWithoutClobberingContent() throws {
        let fileURL = tempDir.appendingPathComponent("history.jsonl")
        try Data("existing-content\n".utf8).write(to: fileURL)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: fileURL.path)

        _ = HistoryService(fileURL: fileURL)

        XCTAssertEqual(try permissions(of: fileURL), 0o600)
        XCTAssertEqual(try String(contentsOf: fileURL, encoding: .utf8), "existing-content\n")
    }

    // MARK: - Write-side bound

    func testRecordTrimsFileToMaxEventsOnWrite() throws {
        let fileURL = tempDir.appendingPathComponent("history.jsonl")
        let service = HistoryService(fileURL: fileURL)

        for i in 0..<1005 {
            try service.record(makeEvent(index: i))
        }

        let contents = try String(contentsOf: fileURL, encoding: .utf8)
        let lines = contents.components(separatedBy: .newlines).filter { !$0.isEmpty }
        XCTAssertEqual(lines.count, 1000, "File on disk must never exceed the 1000-event bound")
        XCTAssertTrue(contents.contains("\"message\":\"m1004\""), "Newest event must be present")
        XCTAssertFalse(contents.contains("\"message\":\"m0\""), "Oldest event must be trimmed")
        XCTAssertTrue(lines.first!.contains("\"message\":\"m5\""), "Oldest kept line must be m5")
        XCTAssertTrue(lines.last!.contains("\"message\":\"m1004\""), "Newest line must be last (order preserved)")
        XCTAssertEqual(try permissions(of: fileURL), 0o600, "Trim rewrite must keep 0600 permissions")
    }

    func testEventsReturnsLastMaxEventsAfterTrim() throws {
        let fileURL = tempDir.appendingPathComponent("history.jsonl")
        let service = HistoryService(fileURL: fileURL)

        for i in 0..<1005 {
            try service.record(makeEvent(index: i))
        }

        let events = try service.events()
        XCTAssertEqual(events.count, 1000)
        XCTAssertEqual(events.first?.message, "m5")
        XCTAssertEqual(events.last?.message, "m1004")
    }
}
