import AppKit
import CoreGraphics
import Foundation

// MARK: - Screen Service

public protocol ScreenServiceProtocol {
    func listDisplays() -> [DisplayInfo]
    func captureMainDisplay(maxDimension: CGFloat) throws -> Snapshot
    func captureDisplay(displayID: UInt32, maxDimension: CGFloat) throws -> Snapshot
    func captureWindow(windowID: Int, maxDimension: CGFloat) throws -> Snapshot
}

extension ScreenServiceProtocol {
    /// Capture the main display using the standard max-dimension limit.
    public func captureMainDisplay() throws -> Snapshot {
        try captureMainDisplay(maxDimension: 1280)
    }

    /// Capture a specific display using the standard max-dimension limit.
    public func captureDisplay(displayID: UInt32) throws -> Snapshot {
        try captureDisplay(displayID: displayID, maxDimension: 1280)
    }

    /// Capture a specific window using the standard max-dimension limit.
    public func captureWindow(windowID: Int) throws -> Snapshot {
        try captureWindow(windowID: windowID, maxDimension: 1280)
    }
}

// MARK: - Accessibility Service

public protocol AccessibilityServiceProtocol {
    func queryFrontmostUI(snapshotID: String, maxDepth: Int, maxNodes: Int) throws -> [UINode]
    func queryUI(
        snapshotID: String,
        processID: Int32,
        windowID: Int,
        windowBounds: RectValue,
        maxDepth: Int,
        maxNodes: Int
    ) throws -> [UINode]
    func resolveElement(snapshotID: String, elementID: String) -> AccessibilityService.ResolvedElement?
    func performElementAction(snapshotID: String, elementID: String, action: String) throws
    func resolveElementAtPoint(x: Double, y: Double) -> AccessibilityService.ResolvedElement?
    func hasCachedNodes(for snapshotID: String) -> Bool
    func cachedNodes(for snapshotID: String) -> [UINode]?
    func cachedSnapshot(for snapshotID: String) -> Snapshot?
    func storeSnapshot(_ snapshot: Snapshot, for snapshotID: String)
    func frontmostFocusedElementRole() -> String?
    func frontmostContainsText(_ text: String) -> Bool
    /// Lightweight polling variant: caches the last frontmost PID and confirmed-absent
    /// text strings so repeated polls skip IPC for unchanged elements.
    func frontmostContainsTextPolling(_ text: String) -> Bool
    /// Reset the polling cache (e.g. when the frontmost application changes).
    func invalidatePollingCache()
    func performMenuAction(path: [String]) throws
    func performMenuAction(path: [String], processID: Int32) throws
    func containsText(_ text: String, processID: Int32) -> Bool
}

extension AccessibilityServiceProtocol {
    public func queryUI(
        snapshotID: String,
        processID: Int32,
        windowID: Int,
        windowBounds: RectValue,
        maxDepth: Int,
        maxNodes: Int
    ) throws -> [UINode] {
        throw AutomationError.unavailable("The accessibility service cannot resolve window \(windowID) for process \(processID).")
    }

    public func performElementAction(snapshotID: String, elementID: String, action: String) throws {
        throw AutomationError.unsupported("Semantic route '\(action)' is unavailable for this accessibility service.")
    }

    public func frontmostContainsTextPolling(_ text: String) -> Bool {
        frontmostContainsText(text)
    }

    public func invalidatePollingCache() {}

    public func performMenuAction(path: [String], processID: Int32) throws {
        throw AutomationError.unavailable("The accessibility service cannot resolve process \(processID) for a targeted menu action.")
    }

    public func containsText(_ text: String, processID: Int32) -> Bool { false }
}

// MARK: - Input Service

public protocol InputServiceProtocol {
    func click(at point: PointValue, button: String, doubleClick: Bool) throws
    func typeText(_ text: String) throws
    func pressKeys(_ keys: [String]) throws
    func scroll(deltaX: Double, deltaY: Double) throws
    func drag(from start: PointValue, to end: PointValue, steps: Int) throws

    func click(at point: PointValue, button: String, doubleClick: Bool, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws
    func typeText(_ text: String, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws
    func pressKeys(_ keys: [String], deliveryMode: DeliveryMode, targetProcessID: Int32?) throws
    func scroll(deltaX: Double, deltaY: Double, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws
    func drag(from start: PointValue, to end: PointValue, steps: Int, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws
}

public extension InputServiceProtocol {
    func click(at point: PointValue, button: String, doubleClick: Bool, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for click requires an input service with a verified target process.")
        }
        try click(at: point, button: button, doubleClick: doubleClick)
    }

    func typeText(_ text: String, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for type_text is unsupported without a verified target process.")
        }
        try typeText(text)
    }

    func pressKeys(_ keys: [String], deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for press_keys is unsupported without a verified target process.")
        }
        try pressKeys(keys)
    }

    func scroll(deltaX: Double, deltaY: Double, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for scroll is unsupported without a verified target process.")
        }
        try scroll(deltaX: deltaX, deltaY: deltaY)
    }

    func drag(from start: PointValue, to end: PointValue, steps: Int, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for drag requires an input service with a verified target process.")
        }
        try drag(from: start, to: end, steps: steps)
    }
}

// MARK: - App Service

public protocol AppServiceProtocol {
    func listApps() -> [AppInfo]
    func listWindows() -> [WindowInfo]
    func frontmostApp() -> AppInfo?
    /// Returns the topmost matching window when the platform exposes ordering.
    func frontmostWindow(ownerPID: Int32, title: String?) -> WindowInfo?
    /// Resolves an explicit identity. Implementations must not use frontmost fallback.
    func resolveTarget(_ target: TargetIdentity) throws -> ResolvedTarget
    func launchApp(bundleID: String?, appName: String?) throws
    func focusWindow(bundleID: String?, appName: String?, title: String?) throws
    /// Resolves and focuses an explicit identity.
    func focusTarget(_ target: TargetIdentity) throws -> ResolvedTarget
}

extension AppServiceProtocol {
    public func resolveTarget(_ target: TargetIdentity) throws -> ResolvedTarget {
        try TargetResolver.resolve(target, apps: listApps(), windows: listWindows(), frontmostWindow: { pid, title in
            frontmostWindow(ownerPID: pid, title: title)
        })
    }

    public func focusTarget(_ target: TargetIdentity) throws -> ResolvedTarget {
        let resolved = try resolveTarget(target)
        try focusWindow(
            bundleID: resolved.app.bundleIdentifier,
            appName: resolved.app.localizedName,
            title: resolved.window?.title
        )
        return resolved
    }

    public func frontmostWindow(ownerPID: Int32, title: String? = nil) -> WindowInfo? {
        listWindows().first { window in
            guard window.ownerPID == ownerPID else { return false }
            guard let title, !title.isEmpty else { return true }
            return window.title?.localizedCaseInsensitiveContains(title) == true
        }
    }
}

// MARK: - OCR Service

public protocol OCRServiceProtocol {
    func recognizeText(in image: CGImage) -> OCRResult
    func isAXTreeWeak(nodeCount: Int, threshold: Int) -> Bool
}

extension OCRServiceProtocol {
    public func isAXTreeWeak(nodeCount: Int) -> Bool {
        isAXTreeWeak(nodeCount: nodeCount, threshold: 3)
    }
}

// MARK: - UI Query Service

public protocol UIQueryServiceProtocol {
    func findNodes(in nodes: [UINode], predicate: UIElementPredicate) -> [UINode]
}

// MARK: - Permission Service

public protocol PermissionServiceProtocol {
    func status() -> PermissionSnapshot
    func requestAccessibilityPermission() -> Bool
    func requestScreenRecordingPermission() -> Bool
}

// MARK: - History Service

public protocol HistoryServiceProtocol {
    func record(_ event: HistoryEvent) throws
    func events() throws -> [HistoryEvent]
}
