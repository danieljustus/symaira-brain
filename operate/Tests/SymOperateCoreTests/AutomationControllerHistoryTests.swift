import XCTest
@testable import SymOperateCore

private final class MockHistoryService: HistoryServiceProtocol {
    var recordedEvents: [HistoryEvent] = []

    func record(_ event: HistoryEvent) throws {
        recordedEvents.append(event)
    }

    func events() throws -> [HistoryEvent] {
        return recordedEvents
    }
}

private final class MockInputServiceForHistory: InputServiceProtocol {
    func click(at point: PointValue, button: String, doubleClick: Bool) throws {}
    func typeText(_ text: String) throws {}
    func pressKeys(_ keys: [String]) throws {}
    func scroll(deltaX: Double, deltaY: Double) throws {}
    func drag(from start: PointValue, to end: PointValue, steps: Int) throws {}
}

private final class MockAccessibilityServiceForHistory: AccessibilityServiceProtocol {
    var focusedRoleOverride: String?

    func queryFrontmostUI(snapshotID: String, maxDepth: Int, maxNodes: Int) throws -> [UINode] { [] }
    func resolveElement(snapshotID: String, elementID: String) -> AccessibilityService.ResolvedElement? { nil }
    func resolveElementAtPoint(x: Double, y: Double) -> AccessibilityService.ResolvedElement? { nil }
    func hasCachedNodes(for snapshotID: String) -> Bool { false }
    func cachedNodes(for snapshotID: String) -> [UINode]? { nil }
    func cachedSnapshot(for snapshotID: String) -> Snapshot? { nil }
    func storeSnapshot(_ snapshot: Snapshot, for snapshotID: String) {}
    func frontmostFocusedElementRole() -> String? { focusedRoleOverride }
    func frontmostContainsText(_ text: String) -> Bool { false }
    func performMenuAction(path: [String]) throws {}
}

private final class MockScreenServiceForHistory: ScreenServiceProtocol {
    func listDisplays() -> [DisplayInfo] { [] }
    func captureMainDisplay(maxDimension: CoreGraphics.CGFloat) throws -> Snapshot { throw AutomationError.unavailable("mock") }
    func captureDisplay(displayID: UInt32, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot { throw AutomationError.unavailable("mock") }
    func captureWindow(windowID: Int, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot { throw AutomationError.unavailable("mock") }
}

private final class MockAppServiceForHistory: AppServiceProtocol {
    func listApps() -> [AppInfo] { [] }
    func listWindows() -> [WindowInfo] { [] }
    func frontmostApp() -> AppInfo? { nil }
    func launchApp(bundleID: String?, appName: String?) throws {}
    func focusWindow(bundleID: String?, appName: String?, title: String?) throws {}
}

private final class MockOCRServiceForHistory: OCRServiceProtocol {
    func recognizeText(in image: CoreGraphics.CGImage) -> OCRResult { OCRResult(regions: [], fullText: "") }
    func isAXTreeWeak(nodeCount: Int, threshold: Int) -> Bool { false }
}

private final class MockUIQueryServiceForHistory: UIQueryServiceProtocol {
    func findNodes(in nodes: [UINode], predicate: UIElementPredicate) -> [UINode] { [] }
}

private let testPermissionSource = PermissionSource(
    pid: 1, ppid: 0, executablePath: "/usr/local/bin/symoperate",
    launchingProcessName: "test",
    note: "Mock source for test."
)

private final class MockPermissionServiceForHistory: PermissionServiceProtocol {
    func status() -> PermissionSnapshot { PermissionSnapshot(accessibilityGranted: true, screenRecordingGranted: true, source: testPermissionSource) }
    func requestAccessibilityPermission() -> Bool { true }
    func requestScreenRecordingPermission() -> Bool { true }
}

final class AutomationControllerHistoryTests: XCTestCase {

    private var controller: AutomationController!
    private var mockHistory: MockHistoryService!
    private var mockAX: MockAccessibilityServiceForHistory!

    override func setUp() {
        super.setUp()
        mockHistory = MockHistoryService()
        mockAX = MockAccessibilityServiceForHistory()
        controller = AutomationController(
            permissions: MockPermissionServiceForHistory(),
            screen: MockScreenServiceForHistory(),
            apps: MockAppServiceForHistory(),
            accessibility: mockAX,
            input: MockInputServiceForHistory(),
            ocr: MockOCRServiceForHistory(),
            queryService: MockUIQueryServiceForHistory(),
            history: mockHistory
        )
    }

    override func tearDown() {
        controller = nil
        mockHistory = nil
        mockAX = nil
        super.tearDown()
    }

    func testTypeTextSuccessRecordsRedactedEvent() throws {
        _ = try controller.typeText("mySecretPassword")

        XCTAssertEqual(mockHistory.recordedEvents.count, 1)
        let event = mockHistory.recordedEvents.first!
        XCTAssertEqual(event.action, "type_text")
        XCTAssertTrue(event.success)
        XCTAssertEqual(event.targets["text"], "<redacted: 16 chars>")
    }

    func testTypeTextRefusedRecordsFailedEvent() throws {
        mockAX.focusedRoleOverride = "AXSecureTextField"

        do {
            _ = try controller.typeText("mySecretPassword")
            XCTFail("Should have thrown")
        } catch {
            XCTAssertEqual(mockHistory.recordedEvents.count, 1)
            let event = mockHistory.recordedEvents.first!
            XCTAssertEqual(event.action, "type_text")
            XCTAssertFalse(event.success)
            XCTAssertEqual(event.targets["text"], "<redacted: 16 chars>")
            XCTAssertTrue(event.message.contains("Refusing to type into a secure text field"))
        }
    }

    func testRawClickFailureRecordsFailedEvent() throws {
        do {
            _ = try controller.click(x: 100, y: 100)
            XCTFail("Should have thrown due to unresolvable coordinates")
        } catch {
            XCTAssertEqual(mockHistory.recordedEvents.count, 1)
            let event = mockHistory.recordedEvents.first!
            XCTAssertEqual(event.action, "click")
            XCTAssertFalse(event.success)
            XCTAssertEqual(event.targets["x"], "100.0")
            XCTAssertEqual(event.targets["y"], "100.0")
            XCTAssertTrue(event.message.contains("Cannot identify the element"))
        }
    }
}
