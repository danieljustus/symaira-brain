import AppKit
import ApplicationServices
import Foundation

public struct InputService: InputServiceProtocol {
    public init() {}

    public func click(at point: PointValue, button: String = "left", doubleClick: Bool = false) throws {
        try click(at: point, button: button, doubleClick: doubleClick, deliveryMode: .automatic, targetProcessID: nil)
    }

    public func click(at point: PointValue, button: String, doubleClick: Bool, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        let mouseButton = try parseMouseButton(button)
        let downType: CGEventType = (mouseButton == .left) ? .leftMouseDown : .rightMouseDown
        let upType: CGEventType = (mouseButton == .left) ? .leftMouseUp : .rightMouseUp

        try postMouseEvent(type: .mouseMoved, point: point, button: mouseButton, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
        for index in 0..<(doubleClick ? 2 : 1) {
            try postMouseEvent(type: downType, point: point, button: mouseButton, clickState: Int64(index + 1), deliveryMode: deliveryMode, targetProcessID: targetProcessID)
            try postMouseEvent(type: upType, point: point, button: mouseButton, clickState: Int64(index + 1), deliveryMode: deliveryMode, targetProcessID: targetProcessID)
            Thread.sleep(forTimeInterval: 0.05)
        }
    }

    public func drag(from start: PointValue, to end: PointValue, steps: Int = 24) throws {
        try drag(from: start, to: end, steps: steps, deliveryMode: .automatic, targetProcessID: nil)
    }

    public func drag(from start: PointValue, to end: PointValue, steps: Int, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        try postMouseEvent(type: .leftMouseDown, point: start, button: .left, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
        let stepCount = max(steps, 2)
        for step in 1...stepCount {
            let t = Double(step) / Double(stepCount)
            let x = start.x + ((end.x - start.x) * t)
            let y = start.y + ((end.y - start.y) * t)
            try postMouseEvent(type: .leftMouseDragged, point: PointValue(x: x, y: y), button: .left, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
            Thread.sleep(forTimeInterval: 0.01)
        }
        try postMouseEvent(type: .leftMouseUp, point: end, button: .left, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
    }

    public func scroll(deltaX: Double = 0, deltaY: Double) throws {
        try scroll(deltaX: deltaX, deltaY: deltaY, deliveryMode: .automatic, targetProcessID: nil)
    }

    public func scroll(deltaX: Double, deltaY: Double, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for scroll is unsupported without a verified target process.")
        }
        // MCP clients can supply arbitrary JSON numbers. Validate both deltas
        // before converting so malformed input cannot trap the server process.
        let wheel1 = try validatedScrollDelta(deltaY, name: "delta_y")
        let wheel2 = try validatedScrollDelta(deltaX, name: "delta_x")

        guard let event = CGEvent(
            scrollWheelEvent2Source: nil,
            units: .pixel,
            wheelCount: 2,
            wheel1: wheel1,
            wheel2: wheel2,
            wheel3: 0
        ) else {
            throw AutomationError.operationFailed("Failed to create a scroll event.")
        }
        try post(event, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
    }

    private func validatedScrollDelta(_ value: Double, name: String) throws -> Int32 {
        guard value.isFinite else {
            throw AutomationError.invalidArgument("Scroll \(name) must be finite.")
        }
        guard value >= Double(Int32.min), value <= Double(Int32.max) else {
            throw AutomationError.invalidArgument("Scroll \(name) must be within the Int32 range.")
        }

        // Preserve the old truncation-toward-zero behavior for fractional
        // in-range values while using a failable, non-trapping conversion.
        guard let converted = Int32(exactly: value.rounded(.towardZero)) else {
            throw AutomationError.invalidArgument("Scroll \(name) must be within the Int32 range.")
        }
        return converted
    }

    public func typeText(_ text: String) throws {
        try typeText(text, deliveryMode: .automatic, targetProcessID: nil)
    }

    public func typeText(_ text: String, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        if deliveryMode == .background && (targetProcessID == nil || targetProcessID! <= 0) {
            throw AutomationError.unsupported("Background delivery for type_text requires a verified target process.")
        }

        // CGEvent's unicode payload is attached to one key event pair. Iterate
        // over Swift Characters, rather than Unicode scalars, so combining
        // marks and emoji ZWJ sequences arrive as one composed insertion.
        for cluster in UnicodeInput.graphemeClusters(in: text) {
            var utf16 = Array(cluster.utf16)
            guard utf16.count <= UnicodeInput.maxUTF16UnitsPerEvent else {
                throw AutomationError.unsupported(
                    "Unicode grapheme cluster cannot be delivered as one keyboard event (\(utf16.count) UTF-16 units)."
                )
            }
            guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true),
                  let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) else {
                throw AutomationError.operationFailed("Failed to create keyboard events.")
            }

            down.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: &utf16)
            up.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: &utf16)
            try postKeyboardEvent(down, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
            try postKeyboardEvent(up, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
        }
    }

    public func pressKeys(_ keys: [String]) throws {
        try pressKeys(keys, deliveryMode: .automatic, targetProcessID: nil)
    }

    public func pressKeys(_ keys: [String], deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        if deliveryMode == .background && (targetProcessID == nil || targetProcessID! <= 0) {
            throw AutomationError.unsupported("Background delivery for press_keys requires a verified target process.")
        }
        guard !keys.isEmpty else {
            throw AutomationError.invalidArgument("press_keys requires at least one key.")
        }

        let parsed = KeyboardShortcut.parse(keys)
        switch parsed.resolution {
        case let .keyCode(keyCode):
            try postKeyCode(keyCode, flags: parsed.flags, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
        case let .unknownKey(key):
            // `press_keys` is a named-key API. Never reinterpret a misspelled
            // or unsupported name as text; callers should use type_text for
            // literal Unicode input.
            throw AutomationError.invalidArgument("Unsupported key '\(key)' in press_keys.")
        case let .multipleKeys(terminals):
            throw AutomationError.invalidArgument(
                "press_keys accepts one non-modifier key; received \(terminals.joined(separator: ", "))."
            )
        case .missingKey:
            throw AutomationError.invalidArgument("press_keys requires a non-modifier key.")
        }
    }

    private func postMouseEvent(
        type: CGEventType,
        point: PointValue,
        button: CGMouseButton,
        clickState: Int64 = 1,
        deliveryMode: DeliveryMode,
        targetProcessID: Int32?
    ) throws {
        guard let event = CGEvent(
            mouseEventSource: nil,
            mouseType: type,
            mouseCursorPosition: CGPoint(x: point.x, y: point.y),
            mouseButton: button
        ) else {
            throw AutomationError.operationFailed("Failed to create a mouse event.")
        }
        event.setIntegerValueField(.mouseEventClickState, value: clickState)
        try post(event, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
    }

    private func postKeyCode(_ keyCode: CGKeyCode, flags: CGEventFlags, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: false) else {
            throw AutomationError.operationFailed("Failed to create keycode events.")
        }

        down.flags = flags
        up.flags = flags
        try postKeyboardEvent(down, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
        try postKeyboardEvent(up, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
    }

    private func postKeyboardEvent(_ event: CGEvent, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        try post(event, deliveryMode: deliveryMode, targetProcessID: targetProcessID)
    }

    private func post(_ event: CGEvent, deliveryMode: DeliveryMode, targetProcessID: Int32?) throws {
        switch deliveryMode {
        case .automatic, .foreground:
            event.post(tap: .cghidEventTap)
        case .background:
            guard let targetProcessID, targetProcessID > 0 else {
                throw AutomationError.unsupported("Background delivery requires a verified target process.")
            }
            event.postToPid(pid_t(targetProcessID))
        }
    }

    private func parseMouseButton(_ value: String) throws -> CGMouseButton {
        switch value.lowercased() {
        case "left":
            return .left
        case "right":
            return .right
        default:
            throw AutomationError.invalidArgument("Unsupported mouse button '\(value)'.")
        }
    }
}

/// The payloads sent through `keyboardSetUnicodeString`.
///
/// Kept as a small named helper so the event boundary is explicit and can be
/// tested without posting events to the user's desktop.
enum UnicodeInput {
    static let maxUTF16UnitsPerEvent = 20

    static func graphemeClusters(in text: String) -> [String] {
        text.map(String.init)
    }
}

struct KeyboardShortcut {
    enum Resolution: Equatable {
        case keyCode(CGKeyCode)
        case unknownKey(String)
        case multipleKeys([String])
        case missingKey
    }

    let flags: CGEventFlags
    let resolution: Resolution

    var keyCode: CGKeyCode? {
        guard case let .keyCode(keyCode) = resolution else { return nil }
        return keyCode
    }

    /// Retained as a nil-valued compatibility property: named-key input never
    /// falls back to typing text. Literal Unicode belongs to `type_text`.
    var fallbackText: String? { nil }

    var unsupportedKey: String? {
        guard case let .unknownKey(key) = resolution else { return nil }
        return key
    }

    static func parse(_ keys: [String]) -> KeyboardShortcut {
        var flags: CGEventFlags = []
        var terminals: [String] = []

        for rawKey in keys {
            let key = normalize(rawKey)
            switch key {
            case "cmd", "command":
                flags.insert(.maskCommand)
            case "ctrl", "control":
                flags.insert(.maskControl)
            case "alt", "option", "opt":
                flags.insert(.maskAlternate)
            case "shift":
                flags.insert(.maskShift)
            default:
                terminals.append(key)
            }
        }

        guard terminals.count == 1 else {
            if terminals.isEmpty {
                return KeyboardShortcut(flags: flags, resolution: .missingKey)
            }
            return KeyboardShortcut(flags: flags, resolution: .multipleKeys(terminals))
        }

        let terminal = terminals[0]
        guard let keyCode = keyCode(for: terminal) else {
            return KeyboardShortcut(flags: flags, resolution: .unknownKey(terminal))
        }
        return KeyboardShortcut(flags: flags, resolution: .keyCode(keyCode))
    }

    static func keyCode(for key: String) -> CGKeyCode? {
        keyCodes[normalize(key)]
    }

    private static func normalize(_ key: String) -> String {
        key.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: "-", with: "_")
            .replacingOccurrences(of: " ", with: "_")
    }

    private static let keyCodes: [String: CGKeyCode] = {
        var map: [String: CGKeyCode] = [
            // ANSI character keys.
            "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9,
            "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19,
            "3": 20, "4": 21, "6": 22, "5": 23, "=": 24, "9": 25, "7": 26, "-": 27, "8": 28,
            "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34, "p": 35, "l": 37, "j": 38,
            "'": 39, "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "n": 45, "m": 46, ".": 47,
            "grave": 50, "backtick": 50,

            // Editing, navigation and modifier keys that are meaningful as a
            // standalone named key. Modifiers used in shortcuts are parsed as
            // flags above, preserving the existing modifier-only validation.
            "tab": 48, "space": 49, "return": 36, "newline": 36, "linefeed": 36,
            "enter": 76, "keypad_enter": 76, "keypadenter": 76, "numpad_enter": 76, "numpadenter": 76, "escape": 53, "esc": 53,
            "delete": 51, "backspace": 51, "forward_delete": 117, "forwarddelete": 117,
            "caps_lock": 57, "capslock": 57, "fn": 63, "function": 63,
            "up": 126, "arrow_up": 126, "arrowup": 126, "up_arrow": 126,
            "down": 125, "arrow_down": 125, "arrowdown": 125, "down_arrow": 125,
            "left": 123, "arrow_left": 123, "arrowleft": 123, "left_arrow": 123,
            "right": 124, "arrow_right": 124, "arrowright": 124, "right_arrow": 124,
            "home": 115, "end": 119, "page_up": 116, "pageup": 116,
            "page_down": 121, "pagedown": 121, "help": 114, "insert": 114,
            "clear": 71,

            // Keypad operators and digits. Bare digits intentionally remain
            // the ANSI number row; keypad input uses an explicit keypad name.
            "keypad_decimal": 65, "keypaddecimal": 65, "numpad_decimal": 65, "numpaddecimal": 65, "decimal": 65,
            "keypad_multiply": 67, "keypadmultiply": 67, "numpad_multiply": 67, "numpadmultiply": 67, "multiply": 67,
            "keypad_plus": 69, "keypadplus": 69, "numpad_plus": 69, "numpadplus": 69,
            "keypad_add": 69, "keypadadd": 69, "numpad_add": 69, "numpadadd": 69, "add": 69,
            "keypad_clear": 71, "keypadclear": 71, "numpad_clear": 71, "numpadclear": 71,
            "keypad_divide": 75, "keypaddivide": 75, "numpad_divide": 75, "numpaddivide": 75, "divide": 75,
            "keypad_minus": 78, "keypadminus": 78, "numpad_minus": 78, "numpadminus": 78, "subtract": 78,
            "keypad_equals": 81, "keypadequals": 81, "numpad_equals": 81, "numpadequals": 81,
            "keypad_0": 82, "keypad0": 82, "numpad_0": 82, "numpad0": 82,
            "keypad_1": 83, "keypad1": 83, "numpad_1": 83, "numpad1": 83,
            "keypad_2": 84, "keypad2": 84, "numpad_2": 84, "numpad2": 84,
            "keypad_3": 85, "keypad3": 85, "numpad_3": 85, "numpad3": 85,
            "keypad_4": 86, "keypad4": 86, "numpad_4": 86, "numpad4": 86,
            "keypad_5": 87, "keypad5": 87, "numpad_5": 87, "numpad5": 87,
            "keypad_6": 88, "keypad6": 88, "numpad_6": 88, "numpad6": 88,
            "keypad_7": 89, "keypad7": 89, "numpad_7": 89, "numpad7": 89,
            "keypad_8": 91, "keypad8": 91, "numpad_8": 91, "numpad8": 91,
            "keypad_9": 92, "keypad9": 92, "numpad_9": 92, "numpad9": 92,
        ]

        let functionKeyCodes: [CGKeyCode] = [
            122, 120, 99, 118, 96, 97, 98, 100, 101, 109,
            103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
        ]
        for (index, keyCode) in functionKeyCodes.enumerated() {
            map["f\(index + 1)"] = keyCode
        }
        return map
    }()
}
