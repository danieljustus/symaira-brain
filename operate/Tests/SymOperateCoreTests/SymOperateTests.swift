import XCTest
@testable import SymOperateCore

// MARK: - Mock Services for findUI tests

private final class MockAccessibilityServiceForFindUI: AccessibilityServiceProtocol {
    private var elements: [String: [String: AccessibilityService.ResolvedElement]] = [:]
    private var nodesCache: [String: [UINode]] = [:]
    private var snapshotCache: [String: Snapshot] = [:]

    func queryFrontmostUI(snapshotID: String, maxDepth: Int, maxNodes: Int) throws -> [UINode] { [] }

    func resolveElement(snapshotID: String, elementID: String) -> AccessibilityService.ResolvedElement? {
        elements[snapshotID]?[elementID]
    }

    func resolveElementAtPoint(x: Double, y: Double) -> AccessibilityService.ResolvedElement? {
        nil
    }

    func hasCachedNodes(for snapshotID: String) -> Bool {
        nodesCache[snapshotID] != nil
    }

    func cachedNodes(for snapshotID: String) -> [UINode]? {
        nodesCache[snapshotID]
    }

    func cachedSnapshot(for snapshotID: String) -> Snapshot? {
        snapshotCache[snapshotID]
    }

    func storeSnapshot(_ snapshot: Snapshot, for snapshotID: String) {
        snapshotCache[snapshotID] = snapshot
    }

    func storeNodes(_ nodes: [UINode], for snapshotID: String) {
        nodesCache[snapshotID] = nodes
    }

    func frontmostFocusedElementRole() -> String? { nil }

    func frontmostContainsText(_ text: String) -> Bool { false }

    func performMenuAction(path: [String]) throws {}
}

private final class MockScreenServiceForFindUI: ScreenServiceProtocol {
    var stubbedSnapshot: Snapshot?
    var captureMainDisplayCalled = false

    func listDisplays() -> [DisplayInfo] { [] }
    func captureMainDisplay(maxDimension: CoreGraphics.CGFloat) throws -> Snapshot {
        captureMainDisplayCalled = true
        if let snapshot = stubbedSnapshot {
            return snapshot
        }
        throw AutomationError.unavailable("mock")
    }
    func captureDisplay(displayID: UInt32, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot {
        throw AutomationError.unavailable("mock")
    }
    func captureWindow(windowID: Int, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot {
        throw AutomationError.unavailable("mock")
    }
}

private final class MockAppServiceForFindUI: AppServiceProtocol {
    func listApps() -> [AppInfo] { [] }
    func listWindows() -> [WindowInfo] { [] }
    func frontmostApp() -> AppInfo? { nil }
    func launchApp(bundleID: String?, appName: String?) throws {}
    func focusWindow(bundleID: String?, appName: String?, title: String?) throws {}
}

private final class MockInputServiceForFindUI: InputServiceProtocol {
    func click(at point: PointValue, button: String, doubleClick: Bool) throws {}
    func typeText(_ text: String) throws {}
    func pressKeys(_ keys: [String]) throws {}
    func scroll(deltaX: Double, deltaY: Double) throws {}
    func drag(from start: PointValue, to end: PointValue, steps: Int) throws {}
}

private final class MockOCRServiceForFindUI: OCRServiceProtocol {
    func recognizeText(in image: CoreGraphics.CGImage) -> OCRResult {
        OCRResult(regions: [], fullText: "")
    }
    func isAXTreeWeak(nodeCount: Int, threshold: Int) -> Bool { nodeCount <= threshold }
}

private func testHistoryService() -> HistoryService {
    HistoryService(fileURL: FileManager.default.temporaryDirectory
        .appendingPathComponent("symoperate-find-ui-\(UUID().uuidString).jsonl"))
}

private final class MockUIQueryServiceForFindUI: UIQueryServiceProtocol {
    var stubbedNodes: [UINode]?

    func findNodes(in nodes: [UINode], predicate: UIElementPredicate) -> [UINode] {
        if let stubbed = stubbedNodes {
            return stubbed
        }
        return nodes.filter { predicate.matches(node: $0) }
    }
}

private let testPermissionSource = PermissionSource(
    pid: 1, ppid: 0, executablePath: "/usr/local/bin/symoperate",
    launchingProcessName: "test",
    note: "Mock source for test."
)

private final class MockPermissionServiceForFindUI: PermissionServiceProtocol {
    func status() -> PermissionSnapshot {
        PermissionSnapshot(accessibilityGranted: true, screenRecordingGranted: true, source: testPermissionSource)
    }
    func requestAccessibilityPermission() -> Bool { true }
    func requestScreenRecordingPermission() -> Bool { true }
}

// MARK: - Tests

final class SymOperateTests: XCTestCase {
    // MARK: - findUI snapshot reuse tests

    func testFindUIReusesCachedSnapshotWhenProvided() throws {
        let mockAccessibility = MockAccessibilityServiceForFindUI()
        let mockQueryService = MockUIQueryServiceForFindUI()
        let mockScreen = MockScreenServiceForFindUI()
        let mockApps = MockAppServiceForFindUI()

        let controller = AutomationController(
            permissions: MockPermissionServiceForFindUI(),
            screen: mockScreen,
            apps: mockApps,
            accessibility: mockAccessibility,
            input: MockInputServiceForFindUI(),
            ocr: MockOCRServiceForFindUI(),
            queryService: mockQueryService,
            history: testHistoryService()
        )

        let snapshotID = "test-snapshot-id"
        let testNode = UINode(id: "node-1", role: "AXButton", subrole: nil, title: "Test Button", label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        let testSnapshot = Snapshot(
            id: snapshotID,
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 100, height: 100),
            displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080),
            displayID: 1,
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), imageSize: SizeValue(width: 100, height: 100))
        )

        mockAccessibility.storeSnapshot(testSnapshot, for: snapshotID)
        mockAccessibility.storeNodes([testNode], for: snapshotID)
        mockQueryService.stubbedNodes = [testNode]

        let predicate = UIElementPredicate(role: "AXButton", title: nil, label: nil, value: nil, subrole: nil, actions: nil)
        let result = try controller.findUI(predicate: predicate, snapshotID: snapshotID)

        XCTAssertEqual(result.snapshot.id, snapshotID)
        XCTAssertEqual(result.nodes.count, 1)
        XCTAssertEqual(result.nodes.first?.id, "node-1")
        XCTAssertFalse(mockScreen.captureMainDisplayCalled, "Should not take a new screenshot when snapshot is cached")
    }

    func testFindUITakesFreshSnapshotWhenNotCached() throws {
        let mockAccessibility = MockAccessibilityServiceForFindUI()
        let mockQueryService = MockUIQueryServiceForFindUI()
        let mockScreen = MockScreenServiceForFindUI()
        let mockApps = MockAppServiceForFindUI()

        let controller = AutomationController(
            permissions: MockPermissionServiceForFindUI(),
            screen: mockScreen,
            apps: mockApps,
            accessibility: mockAccessibility,
            input: MockInputServiceForFindUI(),
            ocr: MockOCRServiceForFindUI(),
            queryService: mockQueryService,
            history: testHistoryService()
        )

        let testNode = UINode(id: "node-1", role: "AXButton", subrole: nil, title: "Test Button", label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        mockScreen.stubbedSnapshot = Snapshot(
            id: "new-snapshot-id",
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 100, height: 100),
            displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080),
            displayID: 1,
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), imageSize: SizeValue(width: 100, height: 100))
        )
        mockQueryService.stubbedNodes = [testNode]

        let predicate = UIElementPredicate(role: "AXButton", title: nil, label: nil, value: nil, subrole: nil, actions: nil)
        let result = try controller.findUI(predicate: predicate, snapshotID: "nonexistent-id")

        XCTAssertEqual(result.snapshot.id, "new-snapshot-id")
        XCTAssertEqual(result.nodes.count, 1)
        XCTAssertTrue(mockScreen.captureMainDisplayCalled, "Should take a new screenshot when snapshot is not cached")
    }

    func testFindUITakesFreshSnapshotWhenNoSnapshotIDProvided() throws {
        let mockAccessibility = MockAccessibilityServiceForFindUI()
        let mockQueryService = MockUIQueryServiceForFindUI()
        let mockScreen = MockScreenServiceForFindUI()
        let mockApps = MockAppServiceForFindUI()

        let controller = AutomationController(
            permissions: MockPermissionServiceForFindUI(),
            screen: mockScreen,
            apps: mockApps,
            accessibility: mockAccessibility,
            input: MockInputServiceForFindUI(),
            ocr: MockOCRServiceForFindUI(),
            queryService: mockQueryService,
            history: testHistoryService()
        )

        let testNode = UINode(id: "node-1", role: "AXButton", subrole: nil, title: "Test Button", label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        mockScreen.stubbedSnapshot = Snapshot(
            id: "new-snapshot-id",
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 100, height: 100),
            displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080),
            displayID: 1,
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), imageSize: SizeValue(width: 100, height: 100))
        )
        mockQueryService.stubbedNodes = [testNode]

        let predicate = UIElementPredicate(role: "AXButton", title: nil, label: nil, value: nil, subrole: nil, actions: nil)
        let result = try controller.findUI(predicate: predicate)

        XCTAssertEqual(result.snapshot.id, "new-snapshot-id")
        XCTAssertEqual(result.nodes.count, 1)
        XCTAssertTrue(mockScreen.captureMainDisplayCalled, "Should take a new screenshot when no snapshot_id provided")
    }

    // MARK: - SnapshotTransform tests

    func testSnapshotTransformMapsImageCoordinatesToDisplaySpace() {
        let transform = SnapshotTransform(
            displayID: 1,
            displayBounds: RectValue(x: 0, y: 0, width: 1440, height: 900),
            imageSize: SizeValue(width: 720, height: 450)
        )

        let point = transform.imageToDisplay(point: PointValue(x: 360, y: 225))
        XCTAssertEqual(point.x, 720, accuracy: 0.01)
        XCTAssertEqual(point.y, 450, accuracy: 0.01)
    }

    // MARK: - KeyboardShortcut tests

    func testKeyboardShortcutParsesCommandShortcut() {
        let shortcut = KeyboardShortcut.parse(["cmd", "s"])
        XCTAssertTrue(shortcut.flags.contains(.maskCommand))
        XCTAssertEqual(shortcut.keyCode, 1)
        XCTAssertNil(shortcut.fallbackText)
    }

    func testKeyboardShortcutRejectsUnknownKeyWithoutTextFallback() {
        let shortcut = KeyboardShortcut.parse(["ß"])
        XCTAssertNil(shortcut.keyCode)
        XCTAssertNil(shortcut.fallbackText)
        XCTAssertEqual(shortcut.resolution, .unknownKey("ß"))
    }

    // MARK: - ListDisplays tests

    func testListDisplaysReturnsAtLeastOneDisplay() {
        let service = ScreenService()
        let displays = service.listDisplays()
        XCTAssertFalse(displays.isEmpty, "Should enumerate at least one display")
        XCTAssertTrue(displays.contains { $0.isMain }, "Should identify the main display")
    }

    // MARK: - DisplayInfo Codable tests

    func testDisplayInfoCodable() throws {
        let display = DisplayInfo(displayID: 1, bounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), isMain: true)
        let data = try JSONEncoder().encode(display)
        let decoded = try JSONDecoder().decode(DisplayInfo.self, from: data)
        XCTAssertEqual(decoded.displayID, 1)
        XCTAssertEqual(decoded.bounds.width, 1920)
        XCTAssertTrue(decoded.isMain)
    }

    // MARK: - Snapshot security: debugImagePath omitted from JSON when nil

    func testSnapshotJSONOmitsDebugImagePathWhenNil() throws {
        let snapshot = Snapshot(
            id: "test-id",
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 100, height: 100),
            displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080),
            displayID: 1,
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), imageSize: SizeValue(width: 100, height: 100))
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let data = try encoder.encode(snapshot)
        let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        XCTAssertNil(json["debugImagePath"], "debugImagePath must be absent when nil")
        XCTAssertNotNil(json["id"])
        XCTAssertNotNil(json["imageBase64PNG"])
        XCTAssertNotNil(json["transform"])
    }

    func testSnapshotJSONIncludesDebugImagePathWhenSet() throws {
        let snapshot = Snapshot(
            id: "test-id",
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 100, height: 100),
            displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080),
            displayID: 1,
            debugImagePath: "/tmp/snapshots/abc.png",
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 1920, height: 1080), imageSize: SizeValue(width: 100, height: 100))
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let data = try encoder.encode(snapshot)
        let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        XCTAssertEqual(json["debugImagePath"] as? String, "/tmp/snapshots/abc.png")
    }

    // MARK: - Snapshot cleanup: 5-minute retention

    func testScreenServiceCleanupRemovesOldPNGs() throws {
        let tmpDir = FileManager.default.temporaryDirectory.appendingPathComponent("test-cleanup-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tmpDir) }

        let oldFile = tmpDir.appendingPathComponent("old-snapshot.png")
        let recentFile = tmpDir.appendingPathComponent("recent-snapshot.png")
        let nonPNG = tmpDir.appendingPathComponent("readme.txt")

        try Data("old".utf8).write(to: oldFile)
        let oldAttrs: [FileAttributeKey: Any] = [.creationDate: Date().addingTimeInterval(-600)]
        try FileManager.default.setAttributes(oldAttrs, ofItemAtPath: oldFile.path)

        try Data("recent".utf8).write(to: recentFile)
        try Data("readme".utf8).write(to: nonPNG)

        _ = ScreenService(snapshotDirectory: tmpDir)

        XCTAssertFalse(FileManager.default.fileExists(atPath: oldFile.path), "Old PNG should be cleaned up")
        XCTAssertTrue(FileManager.default.fileExists(atPath: recentFile.path), "Recent PNG should be kept")
        XCTAssertTrue(FileManager.default.fileExists(atPath: nonPNG.path), "Non-PNG files should be kept")
    }

    func testScreenServiceCleanupRetainsPNGsWithinWindow() throws {
        let tmpDir = FileManager.default.temporaryDirectory.appendingPathComponent("test-cleanup-recent-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tmpDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tmpDir) }

        let recentFile = tmpDir.appendingPathComponent("fresh.png")
        try Data("fresh".utf8).write(to: recentFile)

        _ = ScreenService(snapshotDirectory: tmpDir)

        XCTAssertTrue(FileManager.default.fileExists(atPath: recentFile.path), "PNGs within the 5-minute window should be kept")
    }
}
