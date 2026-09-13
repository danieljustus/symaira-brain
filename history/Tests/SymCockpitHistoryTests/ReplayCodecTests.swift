import Foundation
import XCTest
@testable import SymCockpitHistory

final class ReplayCodecTests: XCTestCase {
    private let timestamp = "2026-08-28T10:00:00Z"

    private func event(
        action: String = "click",
        target: [String: HistoryJSONValue] = [
            "bundle_id": .string("com.example.Editor"),
            "role": .string("button"),
            "label": .string("Run")
        ],
        payload: [String: HistoryJSONValue] = [:]
    ) -> CanonicalHistoryEvent {
        var values = payload
        values["target"] = .object(target)
        values["success"] = .bool(true)
        return CanonicalHistoryEvent(
            source: "operate",
            timestamp: timestamp,
            action: action,
            payload: values
        )
    }

    func testCodecRedactsSecretsAndSecureFieldsBeforeEncoding() throws {
        let record = event(payload: [
            "note": .string("authorization: Bearer sk-test-secret-12345"),
            "password": .string("private user content")
        ])

        let data = try DeterministicReplayCodec().encode([record])
        let encoded = String(decoding: data, as: UTF8.self)
        XCTAssertFalse(encoded.contains("sk-test-secret-12345"))
        XCTAssertFalse(encoded.contains("private user content"))
        XCTAssertTrue(encoded.contains("<redacted>"))

        let decoded = try DeterministicReplayCodec().decode(data)
        XCTAssertEqual(decoded.records.count, 1)
        XCTAssertEqual(decoded.records[0].payload["password"], .string("<redacted>"))
    }

    func testCodecIsDeterministicAndPreservesRecordOrder() throws {
        let first = event(payload: ["z": .integer(1), "a": .string("first")])
        let second = event(action: "scroll", payload: ["a": .string("second"), "z": .integer(2)])
        let reorderedFirst = event(payload: ["a": .string("first"), "z": .integer(1)])
        let reorderedSecond = event(action: "scroll", payload: ["z": .integer(2), "a": .string("second")])
        let codec = DeterministicReplayCodec()

        XCTAssertEqual(try codec.encode([first, second]), try codec.encode([reorderedFirst, reorderedSecond]))
        let decoded = try codec.decode(try codec.encode([first, second]))
        XCTAssertEqual(decoded.records.map(\.action), ["click", "scroll"])
        XCTAssertEqual(decoded.records.map { $0.payload["a"] }, [.string("first"), .string("second")])
    }

    func testCodecRefusesUnsafeAndNonDeterministicRecords() throws {
        let codec = DeterministicReplayCodec()

        XCTAssertThrowsError(try codec.encode([event(action: "type_text")])) { error in
            guard case .unsafeRecord = error as? DeterministicReplayError else {
                return XCTFail("Expected unsafe record, got \(error)")
            }
        }

        let stale = event(target: ["element_id": .string("snapshot-element-42")])
        XCTAssertThrowsError(try codec.encode([stale])) { error in
            guard case .nonDeterministicRecord = error as? DeterministicReplayError else {
                return XCTFail("Expected non-deterministic record, got \(error)")
            }
        }

        XCTAssertThrowsError(try codec.encode([event(action: "unknown_action")])) { error in
            guard case .unsupportedAction = error as? DeterministicReplayError else {
                return XCTFail("Expected unsupported action, got \(error)")
            }
        }
    }

    func testCodecBoundsRecordCountAndEncodedSize() throws {
        let records = [event(), event(action: "scroll")]
        XCTAssertThrowsError(try DeterministicReplayCodec(maximumRecords: 1).encode(records)) { error in
            XCTAssertEqual(error as? DeterministicReplayError, .tooManyRecords(actual: 2, maximum: 1))
        }

        XCTAssertThrowsError(try DeterministicReplayCodec(maximumBytes: 1).encode([event()])) { error in
            guard case .encodedSizeExceeded = error as? DeterministicReplayError else {
                return XCTFail("Expected encoded-size limit, got \(error)")
            }
        }
    }

    func testCodecRedactsOpaqueValuesForSensitiveKeyNames() throws {
        let sensitiveKeys = [
            "access_token", "refresh_token", "id_token", "client_secret",
            "user_password", "x-api-key", "session_id"
        ]
        let codec = DeterministicReplayCodec()

        for (index, key) in sensitiveKeys.enumerated() {
            let opaqueValue = "opaque-runtime-value-\(index)"
            let record = event(payload: [key: .string(opaqueValue)])
            let sanitized = try codec.recording(from: [record]).records[0]
            XCTAssertEqual(
                sanitized.payload[key],
                .string("<redacted>"),
                "Expected opaque value under \(key) to be redacted"
            )
        }
    }

    func testCodecDoesNotRedactBenignKeyFragments() throws {
        let record = event(payload: [
            "monkey_business": .string("benign-value"),
            "keyboard_layout": .string("qwerty")
        ])

        let sanitized = try DeterministicReplayCodec().recording(from: [record]).records[0]
        XCTAssertEqual(sanitized.payload["monkey_business"], .string("benign-value"))
        XCTAssertEqual(sanitized.payload["keyboard_layout"], .string("qwerty"))
    }

    func testCodecRedactionIsIdempotentThroughNestedObjectsAndArrays() throws {
        let record = event(payload: [
            "nested": .object([
                "refresh_token": .string("opaque-nested-token"),
                "items": .array([
                    .object(["client_secret": .string("opaque-array-secret")]),
                    .object(["session_id": .string("opaque-array-session")])
                ])
            ]),
            "keyboard_layout": .string("qwerty")
        ])
        let codec = DeterministicReplayCodec()

        let firstEncoding = try codec.encode([record])
        let firstRecording = try codec.decode(firstEncoding)
        let secondEncoding = try codec.encode(firstRecording)

        XCTAssertEqual(firstEncoding, secondEncoding)
        XCTAssertEqual(firstRecording.records[0].payload["keyboard_layout"], .string("qwerty"))
        guard case .object(let nested) = firstRecording.records[0].payload["nested"],
              case .string(let refreshToken) = nested["refresh_token"],
              case .array(let items) = nested["items"],
              case .object(let firstItem) = items[0],
              case .object(let secondItem) = items[1] else {
            return XCTFail("Expected nested replay payload to retain its structure")
        }
        XCTAssertEqual(refreshToken, "<redacted>")
        XCTAssertEqual(firstItem["client_secret"], .string("<redacted>"))
        XCTAssertEqual(secondItem["session_id"], .string("<redacted>"))
    }

    func testDecodeResanitizesHandWrittenEnvelope() throws {
        let rawCredential = ["raw", String(repeating: "credential", count: 2)].joined(separator: "-")
        let envelope: [String: Any] = [
            "schema_version": DeterministicReplayRecording.currentSchemaVersion,
            "records": [[
                "schema_version": CanonicalHistoryEvent.currentSchemaVersion,
                "source": "operate",
                "timestamp": timestamp,
                "action": "click",
                "payload": [
                    "target": ["bundle_id": "com.example.Editor"],
                    "credential": rawCredential,
                    "success": true
                ]
            ]]
        ]
        let data = try JSONSerialization.data(withJSONObject: envelope, options: [.sortedKeys])

        let decoded = try DeterministicReplayCodec().decode(data)

        XCTAssertEqual(decoded.records[0].payload["credential"], .string("<redacted>"))
        XCTAssertFalse(String(decoding: data, as: UTF8.self).contains("<redacted>"))
    }

}
