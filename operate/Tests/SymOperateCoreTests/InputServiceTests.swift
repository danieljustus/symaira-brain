import XCTest
import CoreGraphics
@testable import SymOperateCore

final class InputServiceTests: XCTestCase {
    // A point deep off-screen so posted synthetic mouse events cannot land on
    // (and disturb) any real window during the test run.
    private let offscreen = PointValue(x: -10000, y: -10000)

    // MARK: - click

    func testClickLeftButtonSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.click(at: offscreen, button: "left"))
    }

    func testClickRightButtonSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.click(at: offscreen, button: "right"))
    }

    func testClickCaseInsensitiveButton() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.click(at: offscreen, button: "LEFT"))
    }

    func testClickDoubleClickSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.click(at: offscreen, button: "left", doubleClick: true))
    }

    func testClickInvalidButtonThrows() {
        let service = InputService()
        XCTAssertThrowsError(try service.click(at: offscreen, button: "middle")) { error in
            guard let automationError = error as? AutomationError, case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }

    // MARK: - drag

    func testDragSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.drag(from: offscreen, to: PointValue(x: -9000, y: -9000), steps: 3))
    }

    func testDragClampsStepsBelowMinimum() throws {
        // steps < 2 is clamped to 2 internally — must not crash on a degenerate range.
        let service = InputService()
        XCTAssertNoThrow(try service.drag(from: offscreen, to: PointValue(x: -9000, y: -9000), steps: 0))
    }

    // MARK: - scroll

    func testScrollSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.scroll(deltaX: 1, deltaY: 10))
    }

    func testScrollDefaultDeltaXSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.scroll(deltaY: -5))
    }

    func testScrollAcceptsInRangeFractionalDeltas() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.scroll(deltaX: 12.75, deltaY: -34.5))
    }

    func testScrollRejectsOutOfInt32RangeDeltas() {
        let service = InputService()
        let values = [
            Double(Int32.max) + 1,
            Double(Int32.min) - 1,
            1e100,
            -1e100,
        ]

        for value in values {
            XCTAssertThrowsError(try service.scroll(deltaY: value), "deltaY \(value) should be rejected") { error in
                guard let automationError = error as? AutomationError,
                      case .invalidArgument = automationError else {
                    return XCTFail("expected invalidArgument, got \(error)")
                }
            }
            XCTAssertThrowsError(try service.scroll(deltaX: value, deltaY: 0), "deltaX \(value) should be rejected") { error in
                guard let automationError = error as? AutomationError,
                      case .invalidArgument = automationError else {
                    return XCTFail("expected invalidArgument, got \(error)")
                }
            }
        }
    }

    func testScrollRejectsNonFiniteDeltas() {
        let service = InputService()
        let values = [Double.nan, Double.infinity, -Double.infinity]

        for value in values {
            XCTAssertThrowsError(try service.scroll(deltaY: value), "deltaY \(value) should be rejected") { error in
                guard let automationError = error as? AutomationError,
                      case .invalidArgument = automationError else {
                    return XCTFail("expected invalidArgument, got \(error)")
                }
            }
            XCTAssertThrowsError(try service.scroll(deltaX: value, deltaY: 0), "deltaX \(value) should be rejected") { error in
                guard let automationError = error as? AutomationError,
                      case .invalidArgument = automationError else {
                    return XCTFail("expected invalidArgument, got \(error)")
                }
            }
        }
    }

    // MARK: - typeText

    func testTypeTextSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.typeText("hi"))
    }

    func testTypeTextEmptyStringSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.typeText(""))
    }

    // MARK: - pressKeys

    func testPressKeysEmptyArrayThrows() {
        let service = InputService()
        XCTAssertThrowsError(try service.pressKeys([])) { error in
            guard let automationError = error as? AutomationError, case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }

    func testPressKeysKnownKeyCodeSucceeds() throws {
        let service = InputService()
        XCTAssertNoThrow(try service.pressKeys(["cmd", "a"]))
    }

    func testPressKeysUnknownKeyThrowsInvalidArgument() {
        let service = InputService()
        XCTAssertThrowsError(try service.pressKeys(["f5x"])) { error in
            guard let automationError = error as? AutomationError,
                  case let .invalidArgument(message) = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
            XCTAssertTrue(message.contains("f5x"))
        }
    }

    func testPressKeysMultipleTerminalKeysThrowsInvalidArgument() {
        let service = InputService()
        XCTAssertThrowsError(try service.pressKeys(["a", "b"])) { error in
            guard let automationError = error as? AutomationError,
                  case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }

    func testPressKeysModifierOnlyThrowsUnresolvable() {
        // A pure modifier list with no terminal key can't be resolved to a key code.
        let service = InputService()
        XCTAssertThrowsError(try service.pressKeys(["cmd", "ctrl", "alt", "shift"])) { error in
            guard let automationError = error as? AutomationError, case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }
}

final class KeyboardShortcutTests: XCTestCase {
    func testParseAllModifiers() {
        let parsed = KeyboardShortcut.parse(["cmd", "ctrl", "alt", "shift", "a"])
        XCTAssertTrue(parsed.flags.contains(.maskCommand))
        XCTAssertTrue(parsed.flags.contains(.maskControl))
        XCTAssertTrue(parsed.flags.contains(.maskAlternate))
        XCTAssertTrue(parsed.flags.contains(.maskShift))
        XCTAssertEqual(parsed.keyCode, 0)
        XCTAssertNil(parsed.fallbackText)
    }

    func testParseUnknownKeyIsClassifiedWithoutTextFallback() {
        let parsed = KeyboardShortcut.parse(["unknownkey"])
        XCTAssertNil(parsed.keyCode)
        XCTAssertNil(parsed.fallbackText)
        XCTAssertEqual(parsed.unsupportedKey, "unknownkey")
        XCTAssertEqual(parsed.resolution, .unknownKey("unknownkey"))
    }

    func testParseMultipleTerminalKeysIsRejected() {
        let parsed = KeyboardShortcut.parse(["cmd", "a", "b"])
        XCTAssertEqual(parsed.resolution, .multipleKeys(["a", "b"]))
        XCTAssertNil(parsed.keyCode)
    }

    func testParseNamedSpecialKeys() {
        for name in ["tab", "space", "return", "enter", "escape", "esc", "delete", "backspace", "up", "down", "left", "right"] {
            let parsed = KeyboardShortcut.parse([name])
            XCTAssertNotNil(parsed.keyCode, "expected a key code mapping for '\(name)'")
        }
    }

    func testParseAllFunctionKeys() {
        let expected: [String: CGKeyCode] = [
            "f1": 122, "f2": 120, "f3": 99, "f4": 118, "f5": 96,
            "f6": 97, "f7": 98, "f8": 100, "f9": 101, "f10": 109,
            "f11": 103, "f12": 111, "f13": 105, "f14": 107, "f15": 113,
            "f16": 106, "f17": 64, "f18": 79, "f19": 80, "f20": 90,
        ]
        for (name, keyCode) in expected {
            XCTAssertEqual(KeyboardShortcut.keyCode(for: name), keyCode, "missing mapping for \(name)")
        }
    }

    func testParseNavigationAndKeypadKeys() {
        let expected: [String: CGKeyCode] = [
            "home": 115, "end": 119, "page_up": 116, "page_down": 121,
            "forward_delete": 117, "help": 114, "keypad_decimal": 65,
            "keypad_multiply": 67, "keypad_plus": 69, "keypad_clear": 71,
            "keypad_divide": 75, "keypad_enter": 76, "keypad_minus": 78,
            "keypad_equals": 81, "keypad_0": 82, "keypad_1": 83,
            "keypad_2": 84, "keypad_3": 85, "keypad_4": 86,
            "keypad_5": 87, "keypad_6": 88, "keypad_7": 89,
            "keypad_8": 91, "keypad_9": 92,
        ]
        for (name, keyCode) in expected {
            XCTAssertEqual(KeyboardShortcut.keyCode(for: name), keyCode, "missing mapping for \(name)")
        }
    }
}

final class UnicodeInputTests: XCTestCase {
    func testGraphemeClustersAreOnePayloadPerComposedCharacter() {
        let text = "e\u{301}👩‍💻🇩🇪x"
        XCTAssertEqual(
            UnicodeInput.graphemeClusters(in: text),
            ["e\u{301}", "👩‍💻", "🇩🇪", "x"]
        )
    }

    func testNewlineIsOneGraphemeCluster() {
        XCTAssertEqual(UnicodeInput.graphemeClusters(in: "first\nsecond").count, 12)
    }
}
