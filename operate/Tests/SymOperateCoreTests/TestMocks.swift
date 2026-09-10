import XCTest
@testable import SymOperateCore

// MARK: - Mock Services (shared across test files)

// MARK: - Mock Services

final class MockAccessibilityService: AccessibilityServiceProtocol {
    private var elements: [String: [String: AccessibilityService.ResolvedElement]] = [:]
    private var nodesCache: [String: [UINode]] = [:]
    private var snapshotCache: [String: Snapshot] = [:]
    var focusedRoleOverride: String?
    var frontmostQueryCount = 0
    var targetQueryCount = 0
    var targetProcessID: Int32?
    var targetWindowID: Int?
    var targetNodes: [UINode] = []

    func prepopulate(snapshotID: String, elementID: String, role: String?, title: String?, label: String?, value: String?, frame: RectValue?) {
        let element = AXUIElementCreateApplication(0)
        let resolved = AccessibilityService.ResolvedElement(element: element, frame: frame, role: role, title: title, label: label, value: value)
        elements[snapshotID, default: [:]][elementID] = resolved
    }

    func prepopulateMany(snapshotID: String, elements: [String: AccessibilityService.ResolvedElement]) {
        for (elementID, resolved) in elements {
            self.elements[snapshotID, default: [:]][elementID] = resolved
        }
    }

    func queryFrontmostUI(snapshotID: String, maxDepth: Int, maxNodes: Int) throws -> [UINode] {
        frontmostQueryCount += 1
        return []
    }

    func queryUI(snapshotID: String, processID: Int32, windowID: Int, windowBounds: RectValue, maxDepth: Int, maxNodes: Int) throws -> [UINode] {
        targetQueryCount += 1
        targetProcessID = processID
        targetWindowID = windowID
        return targetNodes
    }

    func resolveElement(snapshotID: String, elementID: String) -> AccessibilityService.ResolvedElement? {
        elements[snapshotID]?[elementID]
    }

    func resolveElementAtPoint(x: Double, y: Double) -> AccessibilityService.ResolvedElement? {
        var bestMatch: AccessibilityService.ResolvedElement?
        var bestArea: Double = .greatestFiniteMagnitude

        for snapshotCache in elements.values {
            for element in snapshotCache.values {
                guard let frame = element.frame else { continue }
                let minX = frame.x
                let maxX = frame.x + frame.width
                let minY = frame.y
                let maxY = frame.y + frame.height
                if x >= minX, x <= maxX, y >= minY, y <= maxY {
                    let area = frame.width * frame.height
                    if area < bestArea {
                        bestArea = area
                        bestMatch = element
                    }
                }
            }
        }
        return bestMatch
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

    func frontmostFocusedElementRole() -> String? { focusedRoleOverride }

    func frontmostContainsText(_ text: String) -> Bool { false }

    func performMenuAction(path: [String]) throws {}
}

final class MockScreenService: ScreenServiceProtocol {
    var stubbedSnapshot: Snapshot?
    var stubbedWindowSnapshot: Snapshot?
    var captureMainDisplayCalled = false
    var capturedWindowIDs: [Int] = []

    func listDisplays() -> [DisplayInfo] { [] }
    func captureMainDisplay(maxDimension: CoreGraphics.CGFloat) throws -> Snapshot {
        captureMainDisplayCalled = true
        if let snapshot = stubbedSnapshot {
            return snapshot
        }
        throw AutomationError.unavailable("mock")
    }
    func captureDisplay(displayID: UInt32, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot { throw AutomationError.unavailable("mock") }
    func captureWindow(windowID: Int, maxDimension: CoreGraphics.CGFloat) throws -> Snapshot {
        capturedWindowIDs.append(windowID)
        if let snapshot = stubbedWindowSnapshot ?? stubbedSnapshot {
            return snapshot
        }
        throw AutomationError.unavailable("mock")
    }
}

final class MockInputService: InputServiceProtocol {
    func click(at point: PointValue, button: String, doubleClick: Bool) throws {}
    func typeText(_ text: String) throws {}
    func pressKeys(_ keys: [String]) throws {}
    func scroll(deltaX: Double, deltaY: Double) throws {}
    func drag(from start: PointValue, to end: PointValue, steps: Int) throws {}
}

final class MockAppService: AppServiceProtocol {
    var apps: [AppInfo] = []
    var windows: [WindowInfo] = []

    func listApps() -> [AppInfo] { apps }
    func listWindows() -> [WindowInfo] { windows }
    func frontmostApp() -> AppInfo? { apps.first(where: \.isActive) ?? apps.first }
    func launchApp(bundleID: String?, appName: String?) throws {}
    func focusWindow(bundleID: String?, appName: String?, title: String?) throws {}
}

final class MockOCRService: OCRServiceProtocol {
    func recognizeText(in image: CoreGraphics.CGImage) -> OCRResult { OCRResult(regions: [], fullText: "") }
    func isAXTreeWeak(nodeCount: Int, threshold: Int) -> Bool { nodeCount <= threshold }
}

final class MockUIQueryService: UIQueryServiceProtocol {
    var stubbedNodes: [UINode]?

    func findNodes(in nodes: [UINode], predicate: UIElementPredicate) -> [UINode] {
        if let stubbed = stubbedNodes {
            return stubbed
        }
        return nodes.filter { predicate.matches(node: $0) }
    }
}

let sharedTestPermissionSource = PermissionSource(
    pid: 1, ppid: 0, executablePath: "/usr/local/bin/symoperate",
    launchingProcessName: "test",
    note: "Mock source for test."
)

final class MockPermissionService: PermissionServiceProtocol {
    func status() -> PermissionSnapshot { PermissionSnapshot(accessibilityGranted: true, screenRecordingGranted: true, source: sharedTestPermissionSource) }
    func requestAccessibilityPermission() -> Bool { true }
    func requestScreenRecordingPermission() -> Bool { true }
}
