import XCTest
@testable import SymCockpitHistory

final class SecretRedactorTests: XCTestCase {
    private func assembled(_ parts: String...) -> String {
        parts.joined()
    }

    func testRedactsEveryVendorPrefixWithRuntimeAssembledKeys() {
        let suffix = String(repeating: "A", count: 24)
        let prefixes = [
            assembled("s", "k", "-"),
            assembled("g", "h", "p", "_"),
            assembled("g", "i", "t", "h", "u", "b", "_", "p", "a", "t", "_"),
            assembled("g", "l", "p", "a", "t", "-"),
            assembled("x", "o", "x", "b", "-"),
            assembled("x", "o", "x", "a", "-"),
            assembled("x", "o", "x", "p", "-"),
            assembled("x", "o", "x", "r", "-"),
            assembled("x", "o", "x", "s", "-"),
            assembled("A", "I", "z", "a")
        ]
        let keys = prefixes.map { $0 + suffix }
        let input = keys.enumerated().map { "vendor\($0.offset)=\($0.element)" }.joined(separator: " ")

        let redacted = SecretRedactor.redact(input)

        for key in keys {
            XCTAssertFalse(redacted.contains(key), "vendor key must be redacted")
        }
        XCTAssertEqual(
            redacted.components(separatedBy: SecretRedactor.placeholder).count - 1,
            keys.count
        )
    }

    func testRedactsMarkerJWTAndAssignmentForms() {
        let marker = assembled("«redacted:", assembled("s", "k", "-"), "…»")
        let jwtSegment = assembled("e", "y", "J", String(repeating: "B", count: 10))
        let jwt = [jwtSegment, jwtSegment, jwtSegment].joined(separator: ".")
        let assignedValue = String(repeating: "C", count: 12)
        let input = [
            marker,
            jwt,
            assembled("api", "_", "key=", assignedValue),
            assembled("token: '", assignedValue, "'")
        ].joined(separator: " ")

        let redacted = SecretRedactor.redact(input)

        XCTAssertEqual(
            redacted.components(separatedBy: SecretRedactor.placeholder).count - 1,
            4
        )
        XCTAssertFalse(redacted.contains(jwt))
        XCTAssertFalse(redacted.contains(assignedValue))
    }

    func testRedactsNestedJSONStringsAndPreservesScalarValues() {
        let jwtSegment = assembled("e", "y", "J", String(repeating: "D", count: 10))
        let jwt = [jwtSegment, jwtSegment, jwtSegment].joined(separator: ".")
        let value: HistoryJSONValue = .object([
            "message": .string(jwt),
            "nested": .array([
                .string(assembled("Bearer ", String(repeating: "E", count: 12))),
                .object(["marker": .string(assembled("«redacted:legacy»"))])
            ]),
            "integer": .integer(42),
            "number": .number(3.5),
            "bool": .bool(true),
            "null": .null
        ])

        let redacted = SecretRedactor.redact(value)

        guard case .object(let object) = redacted,
              case .array(let nested) = object["nested"],
              case .object(let nestedObject) = nested[1] else {
            return XCTFail("Expected recursively redacted object and array")
        }
        XCTAssertEqual(object["message"], .string(SecretRedactor.placeholder))
        XCTAssertEqual(nested[0], .string(SecretRedactor.placeholder))
        XCTAssertEqual(nestedObject["marker"], .string(SecretRedactor.placeholder))
        XCTAssertEqual(object["integer"], .integer(42))
        XCTAssertEqual(object["number"], .number(3.5))
        XCTAssertEqual(object["bool"], .bool(true))
        XCTAssertEqual(object["null"], .null)
    }
}
