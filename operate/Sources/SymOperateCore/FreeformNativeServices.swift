import AppKit
import ApplicationServices
import Foundation

/// Language-independent AX menu shortcuts. Command is implicit unless NoCommand (8) is set.
enum FreeformMenuShortcut {
    static func matches(character: String?, mask: Int?, command: String, expectedMask: Int) -> Bool {
        character?.lowercased() == command && mask == expectedMask
    }
}

/// Materializes lazy pasteboard providers before overwriting anything. A missing type is an error.
public struct NativeFreeformPasteboard: FreeformPasteboardProtocol {
    public init() {}

    public func snapshot() throws -> FreeformPasteboardSnapshot {
        try (NSPasteboard.general.pasteboardItems ?? []).map { item in
            var types: [String: Data] = [:]
            for type in item.types {
                guard let data = item.data(forType: type) else {
                    throw AutomationError.unavailable("Cannot back up pasteboard type \(type.rawValue).")
                }
                types[type.rawValue] = data
            }
            return types
        }
    }

    public func write(_ data: Data, type: String) throws {
        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.setData(data, forType: NSPasteboard.PasteboardType(type)) else {
            throw AutomationError.unavailable("Could not write Freeform pasteboard payload.")
        }
    }

    public func restore(_ snapshot: FreeformPasteboardSnapshot) -> Bool {
        let items = snapshot.map { types in
            let item = NSPasteboardItem()
            for (type, data) in types { item.setData(data, forType: NSPasteboard.PasteboardType(type)) }
            return item
        }
        NSPasteboard.general.clearContents()
        guard items.isEmpty || NSPasteboard.general.writeObjects(items) else { return false }
        return (try? self.snapshot()) == snapshot
    }
}

/// AX only. No NSRunningApplication.activate, System Events scripts or synthetic key events.
/// Focus uses AXFrontmost, equivalent to System Events' process.frontmost setter.
public struct NativeFreeformEnvironment: FreeformEnvironmentProtocol {
    private let apps: any AppServiceProtocol
    private let permissions: any PermissionServiceProtocol

    public init(apps: any AppServiceProtocol = AppService(), permissions: any PermissionServiceProtocol = PermissionService()) {
        self.apps = apps
        self.permissions = permissions
    }

    public func doctor() -> FreeformDoctor {
        let pid = apps.listApps().first { $0.bundleIdentifier == FreeformDescriptor.bundleID }?.processIdentifier
        let granted = permissions.status().accessibilityGranted
        let windows = pid.flatMap { axCopyElements(AXUIElementCreateApplication($0), attribute: kAXWindowsAttribute) } ?? []
        return FreeformDoctor(running: pid != nil, accessibility: granted, windowCount: windows.count, processID: pid)
    }

    public func frontmostPID() -> Int32? { apps.frontmostApp()?.processIdentifier }

    public func bringToFront(pid: Int32) throws {
        let app = AXUIElementCreateApplication(pid)
        try check(AXUIElementSetAttributeValue(app, kAXFrontmostAttribute as CFString, kCFBooleanTrue), "AXFrontmost")
        for _ in 0..<20 {
            if frontmostPID() == pid { return }
            pause(seconds: 0.025)
        }
        throw AutomationError.unavailable("AXFrontmost did not focus the requested PID.")
    }

    public func pasteEnabled() throws -> Bool {
        guard let paste = try menu(command: "v", modifiers: 0) else { return false }
        return axBoolify(axCopyAttribute(paste, attribute: kAXEnabledAttribute)) == true
    }

    public func deselectIfAvailable() throws {
        if let item = try menu(command: "a", modifiers: 1),
           axBoolify(axCopyAttribute(item, attribute: kAXEnabledAttribute)) == true {
            try check(AXUIElementPerformAction(item, kAXPressAction as CFString), "Deselect AXPress")
        }
    }

    public func pressPaste() throws {
        guard let paste = try menu(command: "v", modifiers: 0),
              axBoolify(axCopyAttribute(paste, attribute: kAXEnabledAttribute)) == true else {
            throw AutomationError.unavailable("Freeform Paste menu item is unavailable.")
        }
        try check(AXUIElementPerformAction(paste, kAXPressAction as CFString), "Paste AXPress")
    }

    public func pause(seconds: Double) { Thread.sleep(forTimeInterval: seconds) }

    public func position(label: String) throws -> PointValue {
        guard let frame = axCopyFrame(try uniqueItem(label)) else {
            throw AutomationError.unavailable("Freeform item has no AX geometry.")
        }
        return PointValue(x: frame.x, y: frame.y)
    }

    public func selectOnly(label: String) throws {
        let target = try uniqueItem(label)
        let items = try canvasItems()
        // AXSelected=false is needed when the localized deselect menu has no shortcut or is disabled in background.
        for item in items where axBoolify(axCopyAttribute(item, attribute: kAXSelectedAttribute)) == true {
            try check(AXUIElementSetAttributeValue(item, kAXSelectedAttribute as CFString, kCFBooleanFalse), "AX deselect")
        }
        try check(AXUIElementSetAttributeValue(target, kAXSelectedAttribute as CFString, kCFBooleanTrue), "AX select")
        for _ in 0..<12 {
            let selected = items.filter { axBoolify(axCopyAttribute($0, attribute: kAXSelectedAttribute)) == true }
            if selected.count == 1, CFEqual(selected[0], target) { return }
            pause(seconds: 0.025)
        }
        throw AutomationError.unavailable("Freeform did not select exactly one target; refusing group movement.")
    }

    public func setPosition(label: String, point: PointValue) throws {
        let target = try uniqueItem(label)
        let selected = try canvasItems().filter { axBoolify(axCopyAttribute($0, attribute: kAXSelectedAttribute)) == true }
        guard selected.count == 1, CFEqual(selected[0], target) else {
            throw AutomationError.unavailable("Selection changed; refusing group movement.")
        }
        var position = CGPoint(x: point.x, y: point.y)
        guard let value = AXValueCreate(.cgPoint, &position) else {
            throw AutomationError.unavailable("Cannot create AXPosition.")
        }
        try check(AXUIElementSetAttributeValue(target, kAXPositionAttribute as CFString, value), "AXPosition")
    }

    private func application() throws -> AXUIElement {
        guard permissions.status().accessibilityGranted else {
            throw AutomationError.permissionDenied("Freeform requires Accessibility permission.")
        }
        guard let app = apps.listApps().first(where: { $0.bundleIdentifier == FreeformDescriptor.bundleID }) else {
            throw AutomationError.notFound("Freeform is not running.")
        }
        let element = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(element, 0.2)
        return element
    }

    /// Cmd is implicit in AXMenuItemCmdModifiers. Shift=1, Option=2, Control=4, NoCommand=8.
    /// Match the complete mask, not merely the command character (avoids Paste and Match Style).
    private func menu(command: String, modifiers: Int) throws -> AXUIElement? {
        guard let bar = axCopyElement(try application(), attribute: kAXMenuBarAttribute) else { return nil }
        let matches = descendants(bar, maxDepth: 5).filter {
            axCopyString($0, attribute: kAXRoleAttribute) == kAXMenuItemRole
                && FreeformMenuShortcut.matches(
                    character: axCopyString($0, attribute: kAXMenuItemCmdCharAttribute),
                    mask: (axCopyAttribute($0, attribute: kAXMenuItemCmdModifiersAttribute) as? NSNumber)?.intValue,
                    command: command, expectedMask: modifiers
                )
        }
        guard matches.count <= 1 else { throw AutomationError.unavailable("Ambiguous Freeform menu shortcut.") }
        return matches.first
    }

    /// Only the focused board window, or the sole window. Never guess between multiple boards.
    private func canvasItems() throws -> [AXUIElement] {
        let app = try application()
        let windows = axCopyElements(app, attribute: kAXWindowsAttribute) ?? []
        guard let window = axCopyElement(app, attribute: kAXFocusedWindowAttribute) ?? (windows.count == 1 ? windows[0] : nil) else {
            throw AutomationError.notFound("No unambiguous Freeform board window.")
        }
        let areas = descendants(window, maxDepth: 5).filter { axCopyString($0, attribute: kAXRoleAttribute) == "AXLayoutArea" }
        guard areas.count == 1 else { throw AutomationError.notFound("No unique Freeform canvas.") }
        return (axCopyElements(areas[0], attribute: kAXChildrenAttribute) ?? []).filter {
            axCopyString($0, attribute: kAXRoleAttribute) == "AXLayoutItem"
        }
    }

    private func uniqueItem(_ label: String) throws -> AXUIElement {
        let matches = try canvasItems().filter {
            guard let description = axCopyString($0, attribute: kAXDescriptionAttribute) else { return false }
            return description == label || description.hasPrefix(label + ", ")
        }
        guard matches.count == 1 else {
            throw AutomationError.notFound("Freeform label must identify exactly one visible canvas object.")
        }
        return matches[0]
    }

    private func descendants(_ root: AXUIElement, maxDepth: Int) -> [AXUIElement] {
        var result: [AXUIElement] = []
        var queue: [(AXUIElement, Int)] = [(root, 0)]
        var index = 0
        while index < queue.count, result.count < 2000 {
            let (element, depth) = queue[index]
            index += 1
            result.append(element)
            if depth < maxDepth {
                queue.append(contentsOf: (axCopyElements(element, attribute: kAXChildrenAttribute) ?? []).prefix(2000).map { ($0, depth + 1) })
            }
        }
        return result
    }

    private func check(_ error: AXError, _ operation: String) throws {
        guard error == .success else { throw AutomationError.unavailable("\(operation) failed: AX \(error.rawValue).") }
    }
}
