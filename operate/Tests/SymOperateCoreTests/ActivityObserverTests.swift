import Foundation
import XCTest
@testable import SymOperateCore

final class ActivityObserverTests: XCTestCase {
    private var rootURL: URL!

    override func setUp() {
        super.setUp()
        rootURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("symcockpit-activity-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: rootURL)
        super.tearDown()
    }

    private func app(_ name: String = "Editor", bundleID: String = "com.example.editor") -> AppInfo {
        AppInfo(localizedName: name, bundleIdentifier: bundleID, processIdentifier: 42, isActive: true)
    }

    private func event(
        at timestamp: Date,
        kind: ActivitySourceEvent.Kind = .foregroundWindow,
        title: String? = "Document",
        bundleID: String = "com.example.editor"
    ) -> ActivitySourceEvent {
        ActivitySourceEvent(
            timestamp: timestamp,
            kind: kind,
            app: app(bundleID: bundleID),
            windowTitle: title,
            windowBounds: ActivityWindowBounds(x: 10, y: 20, width: 800, height: 600)
        )
    }

    func testStoreUsesFixedTenMinuteDirectoriesAndRotates() throws {
        let store = ActivitySegmentStore(rootURL: rootURL)
        let first = Date(timeIntervalSince1970: 1_725_000_001)
        let second = first.addingTimeInterval(601)

        try store.append(event(at: first), policy: ActionPolicy(), now: first)
        try store.append(event(at: second), policy: ActionPolicy(), now: second)

        XCTAssertNotEqual(store.segmentURL(for: first), store.segmentURL(for: second))
        XCTAssertEqual(
            store.segmentURL(for: first).lastPathComponent,
            "2024-08-30T06-40-00Z"
        )
        XCTAssertTrue(FileManager.default.fileExists(atPath: store.segmentURL(for: first).appendingPathComponent("events.jsonl").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: store.segmentURL(for: first).appendingPathComponent("metadata.json").path))
    }

    func testStoreWritesMetadataAndRedactsAtWriteBoundary() throws {
        let store = ActivitySegmentStore(rootURL: rootURL)
        let timestamp = Date(timeIntervalSince1970: 1_725_000_001)
        let sensitive = event(at: timestamp, title: "https://example.test/report?token=secret#details")
        try store.append(sensitive, policy: ActionPolicy(), now: timestamp)

        let segment = store.segmentURL(for: timestamp)
        let metadataData = try Data(contentsOf: segment.appendingPathComponent("metadata.json"))
        let metadata = try JSONDecoder().decode(ActivitySegmentMetadata.self, from: metadataData)
        XCTAssertEqual(metadata.eventCount, 1)
        XCTAssertEqual(metadata.bundleIDs, ["com.example.editor"])
        XCTAssertEqual(metadata.redactionPolicyVersion, ActivityRedactor.policyVersion)
        XCTAssertEqual(metadata.windowBounds.count, 1)

        let events = try store.events(inSegmentFor: timestamp)
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(events[0].source, "activity")
        XCTAssertEqual(events[0].action, ActivitySourceEvent.Kind.foregroundWindow.rawValue)
        XCTAssertEqual(
            events[0].payload["window_title"],
            .string("https://example.test/report")
        )
        let fileText = try String(contentsOf: segment.appendingPathComponent("events.jsonl"), encoding: .utf8)
        XCTAssertFalse(fileText.contains("token=secret"))
        XCTAssertFalse(fileText.contains("#details"))
    }

    func testPolicyAndSecretShapesDropWindowTitlesWithoutCapturingContent() throws {
        let policy = ActionPolicy(extraDenyKeywords: ["internal"])
        XCTAssertNil(ActivityRedactor.redactWindowTitle("Delete this file", policy: policy))
        XCTAssertNil(ActivityRedactor.redactWindowTitle("internal project", policy: policy))
        XCTAssertNil(ActivityRedactor.redactWindowTitle("Password: hunter2", policy: policy))
        XCTAssertNil(ActivityRedactor.redactWindowTitle("sk-abcdefghijklmnop", policy: policy))
        XCTAssertEqual(
            ActivityRedactor.redactWindowTitle("A normal document", policy: policy),
            "A normal document"
        )
    }

    func testHistorySecretRedactorDropsRuntimeAssembledGitHubTokenTitle() {
        let credential = "github" + "_pat_" + String(repeating: "A", count: 20)

        XCTAssertNil(
            ActivityRedactor.redactWindowTitle("Issue \(credential)", policy: ActionPolicy())
        )
    }

    func testRetentionAndMaximumSegmentsBoundDiskState() throws {
        let store = ActivitySegmentStore(rootURL: rootURL, retention: 3600, maxSegments: 2)
        let first = Date(timeIntervalSince1970: 1_725_000_001)
        let second = first.addingTimeInterval(601)
        let third = second.addingTimeInterval(601)

        try store.append(event(at: first), policy: ActionPolicy(), now: first)
        try store.append(event(at: second), policy: ActionPolicy(), now: second)
        try store.append(event(at: third), policy: ActionPolicy(), now: third)

        let directories = try FileManager.default.contentsOfDirectory(at: rootURL, includingPropertiesForKeys: [.isDirectoryKey])
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
        XCTAssertEqual(directories.count, 2)
        XCTAssertFalse(FileManager.default.fileExists(atPath: store.segmentURL(for: first).path))

        let retentionStore = ActivitySegmentStore(rootURL: rootURL.appendingPathComponent("retention"), retention: 60)
        let old = first
        let current = old.addingTimeInterval(601)
        try retentionStore.append(event(at: old), policy: ActionPolicy(), now: old)
        try retentionStore.append(event(at: current), policy: ActionPolicy(), now: current)
        XCTAssertFalse(FileManager.default.fileExists(atPath: retentionStore.segmentURL(for: old).path))
    }

    func testObserverIsInertUntilStartedAndStopsForwarding() throws {
        let source = TestActivityEventSource()
        let store = ActivitySegmentStore(rootURL: rootURL)
        let observer = ActivityObserver(source: source, store: store)
        let timestamp = Date(timeIntervalSince1970: 1_725_000_001)

        source.emit(event(at: timestamp))
        XCTAssertFalse(observer.isRunning)
        XCTAssertEqual(try store.events(inSegmentFor: timestamp).count, 0)

        observer.start()
        XCTAssertTrue(observer.isRunning)
        source.emit(event(at: timestamp))
        XCTAssertEqual(try store.events(inSegmentFor: timestamp).count, 1)
        observer.start()
        XCTAssertEqual(source.startCount, 1)

        observer.stop()
        XCTAssertFalse(observer.isRunning)
        source.emit(event(at: timestamp.addingTimeInterval(1)))
        XCTAssertEqual(try store.events(inSegmentFor: timestamp).count, 1)
        observer.stop()
        XCTAssertEqual(source.stopCount, 1)
    }

    func testDeleteSegmentAndDeleteAllRemoveFiles() throws {
        let store = ActivitySegmentStore(rootURL: rootURL)
        let timestamp = Date(timeIntervalSince1970: 1_725_000_001)
        try store.append(event(at: timestamp), policy: ActionPolicy(), now: timestamp)
        try store.deleteSegment(for: timestamp)
        XCTAssertFalse(FileManager.default.fileExists(atPath: store.segmentURL(for: timestamp).path))

        try store.append(event(at: timestamp), policy: ActionPolicy(), now: timestamp)
        try store.deleteAll()
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: rootURL.path).count, 0)
    }
}

private final class TestActivityEventSource: ActivityEventSource {
    private var handler: (@Sendable (ActivitySourceEvent) -> Void)?
    private(set) var startCount = 0
    private(set) var stopCount = 0

    func start(handler: @escaping @Sendable (ActivitySourceEvent) -> Void) {
        startCount += 1
        self.handler = handler
    }

    func stop() {
        stopCount += 1
        handler = nil
    }

    func emit(_ event: ActivitySourceEvent) {
        handler?(event)
    }
}
