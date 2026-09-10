import XCTest
@testable import SymOperateCore

final class AutomationControllerWindowTargetTests: XCTestCase {
    private let frontmostApp = AppInfo(
        localizedName: "Frontmost",
        bundleIdentifier: "com.example.frontmost",
        processIdentifier: 101,
        isActive: true
    )
    private let backgroundApp = AppInfo(
        localizedName: "Background",
        bundleIdentifier: "com.example.background",
        processIdentifier: 202,
        isActive: false
    )

    private func makeSnapshot(
        id: String,
        windowID: Int? = nil,
        ownerPID: Int32? = nil
    ) -> Snapshot {
        let bounds = RectValue(x: 10, y: 20, width: 800, height: 600)
        let imageSize = SizeValue(width: 10, height: 10)
        return Snapshot(
            id: id,
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "not-an-image",
            imageSize: imageSize,
            displayBounds: bounds,
            displayID: 1,
            windowID: windowID,
            windowOwnerPID: ownerPID,
            transform: SnapshotTransform(displayID: 1, displayBounds: bounds, imageSize: imageSize)
        )
    }

    private func makeWindow() -> WindowInfo {
        WindowInfo(
            windowID: 42,
            ownerName: "Background",
            ownerPID: backgroundApp.processIdentifier,
            title: "Background Window",
            bounds: RectValue(x: 10, y: 20, width: 800, height: 600),
            layer: 0
        )
    }

    private func makeController(
        apps: MockAppService,
        screen: MockScreenService,
        accessibility: MockAccessibilityService
    ) -> AutomationController {
        AutomationController(
            permissions: MockPermissionService(),
            screen: screen,
            apps: apps,
            accessibility: accessibility,
            input: MockInputService(),
            ocr: MockOCRService(),
            queryService: MockUIQueryService(),
            history: HistoryService(fileURL: FileManager.default.temporaryDirectory
                .appendingPathComponent("symoperate-window-target-\(UUID().uuidString).jsonl"))
        )
    }

    func testQueryUIWithoutWindowIDKeepsFrontmostBehavior() throws {
        let apps = MockAppService()
        apps.apps = [frontmostApp, backgroundApp]
        let screen = MockScreenService()
        screen.stubbedSnapshot = makeSnapshot(id: "frontmost")
        let accessibility = MockAccessibilityService()
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)

        let result = try controller.queryUI()

        XCTAssertEqual(result.app?.processIdentifier, frontmostApp.processIdentifier)
        XCTAssertEqual(accessibility.frontmostQueryCount, 1)
        XCTAssertEqual(accessibility.targetQueryCount, 0)
        XCTAssertTrue(screen.captureMainDisplayCalled)
        XCTAssertTrue(screen.capturedWindowIDs.isEmpty)
    }

    func testQueryUIWindowIDUsesOwningProcessAndWindowNotFrontmostApp() throws {
        let apps = MockAppService()
        apps.apps = [frontmostApp, backgroundApp]
        apps.windows = [makeWindow()]
        let screen = MockScreenService()
        screen.stubbedWindowSnapshot = makeSnapshot(
            id: "background-window",
            windowID: makeWindow().windowID,
            ownerPID: backgroundApp.processIdentifier
        )
        let accessibility = MockAccessibilityService()
        accessibility.targetNodes = [UINode(
            id: "background-node",
            role: "AXButton",
            subrole: nil,
            title: "Background Button",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: []
        )]
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)

        let result = try controller.queryUI(windowID: 42)

        XCTAssertEqual(result.app?.processIdentifier, backgroundApp.processIdentifier)
        XCTAssertEqual(result.nodes.first?.title, "Background Button")
        XCTAssertEqual(accessibility.targetQueryCount, 1)
        XCTAssertEqual(accessibility.targetProcessID, backgroundApp.processIdentifier)
        XCTAssertEqual(accessibility.targetWindowID, 42)
        XCTAssertEqual(accessibility.frontmostQueryCount, 0)
        XCTAssertEqual(screen.capturedWindowIDs, [42])
    }

    func testQueryUIWithOCRWindowIDUsesOwningProcessAndWindow() throws {
        let apps = MockAppService()
        apps.apps = [frontmostApp, backgroundApp]
        apps.windows = [makeWindow()]
        let screen = MockScreenService()
        screen.stubbedWindowSnapshot = makeSnapshot(
            id: "background-window-ocr",
            windowID: 42,
            ownerPID: backgroundApp.processIdentifier
        )
        let accessibility = MockAccessibilityService()
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)

        let result = try controller.queryUIWithOCR(windowID: 42)

        XCTAssertEqual(result.app?.processIdentifier, backgroundApp.processIdentifier)
        XCTAssertEqual(accessibility.targetProcessID, backgroundApp.processIdentifier)
        XCTAssertEqual(accessibility.targetWindowID, 42)
        XCTAssertEqual(accessibility.frontmostQueryCount, 0)
    }

    func testQueryUIInvalidWindowIDReturnsNotFoundWithoutCapturingOrFallingBack() {
        let apps = MockAppService()
        apps.apps = [frontmostApp]
        let screen = MockScreenService()
        screen.stubbedSnapshot = makeSnapshot(id: "must-not-be-used")
        let accessibility = MockAccessibilityService()
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)

        XCTAssertThrowsError(try controller.queryUI(windowID: 999)) { error in
            guard let automationError = error as? AutomationError else {
                return XCTFail("Expected AutomationError, got \(error)")
            }
            XCTAssertEqual(automationError.code, "not_found")
            if case .notFound(let message) = automationError {
                XCTAssertTrue(message.contains("999"))
            } else {
                XCTFail("Expected notFound, got \(automationError)")
            }
        }
        XCTAssertTrue(screen.capturedWindowIDs.isEmpty)
        XCTAssertEqual(accessibility.frontmostQueryCount, 0)
    }

    func testFindUIRejectsDisappearedTargetBeforeReusingItsCachedSnapshot() throws {
        let apps = MockAppService()
        apps.apps = [frontmostApp, backgroundApp]
        apps.windows = [makeWindow()]
        let screen = MockScreenService()
        let accessibility = MockAccessibilityService()
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)
        let snapshot = makeSnapshot(id: "stale-target", windowID: 42, ownerPID: backgroundApp.processIdentifier)
        accessibility.storeSnapshot(snapshot, for: snapshot.id)
        accessibility.storeNodes([], for: snapshot.id)
        apps.windows = []

        XCTAssertThrowsError(try controller.findUI(predicate: UIElementPredicate(), snapshotID: snapshot.id, windowID: 42)) { error in
            guard let automationError = error as? AutomationError else {
                return XCTFail("Expected AutomationError, got \(error)")
            }
            XCTAssertEqual(automationError.code, "not_found")
        }
        XCTAssertEqual(accessibility.frontmostQueryCount, 0)
        XCTAssertEqual(accessibility.targetQueryCount, 0)
    }

    func testFindUIDoesNotReuseUnscopedFrontmostSnapshotForWindowTarget() throws {
        let apps = MockAppService()
        apps.apps = [frontmostApp, backgroundApp]
        apps.windows = [makeWindow()]
        let screen = MockScreenService()
        screen.stubbedWindowSnapshot = makeSnapshot(id: "fresh-target", windowID: 42, ownerPID: 202)
        let accessibility = MockAccessibilityService()
        accessibility.storeSnapshot(makeSnapshot(id: "frontmost-cache"), for: "frontmost-cache")
        accessibility.storeNodes([UINode(
            id: "wrong-node",
            role: "AXButton",
            subrole: nil,
            title: "Wrong App",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: []
        )], for: "frontmost-cache")
        accessibility.targetNodes = []
        let controller = makeController(apps: apps, screen: screen, accessibility: accessibility)

        let result = try controller.findUI(predicate: UIElementPredicate(), snapshotID: "frontmost-cache", windowID: 42)

        XCTAssertEqual(result.snapshot.id, "fresh-target")
        XCTAssertEqual(accessibility.targetQueryCount, 1)
        XCTAssertEqual(accessibility.frontmostQueryCount, 0)
        XCTAssertEqual(screen.capturedWindowIDs, [42])
    }

    func testExplicitDisplayAndSpaceScopeSelectsMatchingWindow() throws {
        let windows = [
            WindowInfo(
                windowID: 42,
                ownerName: backgroundApp.localizedName,
                ownerPID: backgroundApp.processIdentifier,
                title: "Document",
                bounds: RectValue(x: 10, y: 20, width: 800, height: 600),
                layer: 0,
                displayID: 1,
                spaceID: 1
            ),
            WindowInfo(
                windowID: 43,
                ownerName: backgroundApp.localizedName,
                ownerPID: backgroundApp.processIdentifier,
                title: "Document",
                bounds: RectValue(x: 1920, y: 20, width: 800, height: 600),
                layer: 0,
                displayID: 2,
                spaceID: 3
            ),
        ]
        let resolved = try TargetResolver.resolve(
            TargetIdentity(processID: backgroundApp.processIdentifier, displayID: 2, spaceID: 3),
            apps: [backgroundApp],
            windows: windows,
            frontmostWindow: { _, _ in nil }
        )

        XCTAssertEqual(resolved.app.processIdentifier, backgroundApp.processIdentifier)
        XCTAssertEqual(resolved.window?.windowID, 43)
        XCTAssertEqual(resolved.window?.displayID, 2)
        XCTAssertEqual(resolved.window?.spaceID, 3)
    }

    func testExplicitDisplayAndSpaceScopeRejectsAmbiguousWindows() {
        let windows = [
            WindowInfo(
                windowID: 42,
                ownerName: backgroundApp.localizedName,
                ownerPID: backgroundApp.processIdentifier,
                title: "Document",
                bounds: RectValue(x: 10, y: 20, width: 800, height: 600),
                layer: 0,
                displayID: 2,
                spaceID: 3
            ),
            WindowInfo(
                windowID: 43,
                ownerName: backgroundApp.localizedName,
                ownerPID: backgroundApp.processIdentifier,
                title: "Other document",
                bounds: RectValue(x: 820, y: 20, width: 800, height: 600),
                layer: 0,
                displayID: 2,
                spaceID: 3
            ),
        ]

        XCTAssertThrowsError(try TargetResolver.resolve(
            TargetIdentity(processID: backgroundApp.processIdentifier, displayID: 2, spaceID: 3),
            apps: [backgroundApp],
            windows: windows,
            frontmostWindow: { _, _ in nil }
        )) { error in
            guard let automationError = error as? AutomationError else {
                return XCTFail("Expected AutomationError, got \(error)")
            }
            XCTAssertEqual(automationError.code, "ambiguous_target")
        }
    }
}
