import XCTest
@testable import SymOperateCore

final class DiagnosticTraceTests: XCTestCase {
    private let outcome = DiagnosticTraceOutcome(
        success: false,
        effect: .refused,
        verification: ActionVerification(
            status: .notAttempted,
            strategy: "policy",
            reason: "authorization token was not accepted",
            checkedAt: "2026-08-28T10:00:01.000Z"
        ),
        message: "Request refused",
        errorCode: "destructive_control_refused"
    )

    private func trace(arguments: [String: String]) -> DiagnosticTraceRecord {
        DiagnosticTraceRecord(
            traceID: "trace-0001",
            action: "type_text",
            startedAt: "2026-08-28T10:00:00.000Z",
            finishedAt: "2026-08-28T10:00:01.000Z",
            request: arguments,
            target: ActionTarget(requestedAppName: "TextEdit", requestedWindowTitle: "Notes"),
            before: DiagnosticTraceObservation(snapshotID: "snapshot-before", nodeCount: 3),
            after: DiagnosticTraceObservation(snapshotID: "snapshot-after", nodeCount: 4),
            route: ActionRouteDiagnostics(
                route: .semantic,
                attempts: [ActionRouteAttempt(route: .semantic, status: .selected, reason: "AXPress was available.")]
            ),
            policy: DiagnosticTracePolicy(decision: "refused", permissions: ["input"]),
            outcome: outcome
        )
    }

    func testSerializationIsStableForDifferentDictionaryInsertionOrder() throws {
        let first = trace(arguments: ["text": "token=«redacted:sk-…»", "elementID": "element-1", "snapshotID": "snapshot-before"])
        let second = trace(arguments: ["snapshotID": "snapshot-before", "elementID": "element-1", "text": "token=«redacted:sk-…»"])

        XCTAssertEqual(try DiagnosticTraceSerializer().encode(first), try DiagnosticTraceSerializer().encode(second))
        XCTAssertEqual(
            try String(decoding: DiagnosticTraceSerializer().encode(first), as: UTF8.self),
            try String(decoding: DiagnosticTraceSerializer().encode(second), as: UTF8.self)
        )
    }

    func testSerializationRedactsSecretsAndExcludesScreenshotData() throws {
        let record = trace(arguments: [
            "text": "my password is hunter2",
            "authorization": "Bearer abcdefghijklmnop",
            "safe": "token=«redacted:sk-…»"
        ])

        let json = try String(decoding: DiagnosticTraceSerializer().encode(record), as: UTF8.self)
        XCTAssertFalse(json.contains("hunter2"))
        XCTAssertFalse(json.contains("abcdefghijklmnop"))
        XCTAssertFalse(json.contains("«redacted:sk-…»"))
        XCTAssertFalse(json.contains("imageBase64PNG"))
        XCTAssertFalse(json.contains("debugImagePath"))
        XCTAssertTrue(json.contains("<redacted>"))
    }

    func testRoundTripDecodesTheVersionedRedactedRecord() throws {
        let record = trace(arguments: ["text": "private value"])
        let serializer = DiagnosticTraceSerializer()
        let data = try serializer.encode(record)
        let decoded = try serializer.decode(data)

        XCTAssertEqual(decoded.schemaVersion, DiagnosticTraceContract.currentVersion)
        XCTAssertEqual(decoded.traceID, record.traceID)
        XCTAssertEqual(decoded.request["text"], "<redacted>")
        XCTAssertEqual(decoded.before?.snapshotID, "snapshot-before")
        XCTAssertEqual(decoded.outcome.errorCode, "destructive_control_refused")
        XCTAssertEqual(try serializer.encode(decoded), data)
    }

    func testHistoryEventCarriesTraceThroughCodable() throws {
        let event = HistoryEvent(
            action: "click",
            success: false,
            message: "Request refused",
            diagnosticTrace: trace(arguments: ["label": "Save"])
        )
        let data = try JSONEncoder().encode(event)
        do {
            let decoded = try JSONDecoder().decode(HistoryEvent.self, from: data)
            XCTAssertEqual(decoded.diagnosticTrace?.traceID, "trace-0001")
        } catch {
            XCTFail("HistoryEvent trace did not decode: \(error)")
        }
    }

    func testBufferRetainsOnlyTheNewestBoundedRecords() {
        var buffer = DiagnosticTraceBuffer(maximumRecords: 2)
        buffer.append(trace(arguments: ["label": "one"]))
        buffer.append(trace(arguments: ["label": "two"]))
        buffer.append(trace(arguments: ["label": "three"]))

        XCTAssertEqual(buffer.records.count, 2)
        XCTAssertEqual(buffer.records.map(\.request), [["label": "two"], ["label": "three"]])
    }

    func testConstructionBufferingAndSerializationRemainRedacted() throws {
        let rawCredential = ["raw", String(repeating: "credential", count: 2)].joined(separator: "-")
        let trace = trace(arguments: ["credential": rawCredential])
        var buffer = DiagnosticTraceBuffer(maximumRecords: 1)

        buffer.append(trace)

        XCTAssertEqual(buffer.records[0].request["credential"], "<redacted>")
        let serialized = try DiagnosticTraceSerializer().encode(buffer.records[0])
        XCTAssertFalse(String(decoding: serialized, as: UTF8.self).contains(rawCredential))
    }

    func testValueRedactionMasksVendorTokensWithinDiagnosticText() {
        let gitLabToken = ["gl", "pat-"].joined() + String(repeating: "g", count: 12)
        let slackToken = ["xo", "xb", "-"].joined() + String(repeating: "s", count: 8)
        let googleToken = ["AI", "za"].joined() + String(repeating: "G", count: 20)
        let diagnostic = "gitlab=\(gitLabToken); slack=\(slackToken); google=\(googleToken)"

        let redacted = trace(arguments: ["diagnostic": diagnostic]).request["diagnostic"]

        XCTAssertEqual(redacted, "gitlab=<redacted>; slack=<redacted>; google=<redacted>")
    }

    func testValueRedactionLeavesDiskCacheTextReadable() {
        let diagnostic = "cache backend: disk-cache is healthy"

        let redacted = trace(arguments: ["diagnostic": diagnostic]).request["diagnostic"]

        XCTAssertEqual(redacted, diagnostic)
    }

    func testDecodeResanitizesHandWrittenEnvelope() throws {
        let rawCredential = ["raw", String(repeating: "credential", count: 2)].joined(separator: "-")
        let envelope: [String: Any] = [
            "schemaVersion": DiagnosticTraceContract.currentVersion,
            "traceID": "trace-raw",
            "action": "click",
            "startedAt": "2026-08-28T10:00:00.000Z",
            "finishedAt": "2026-08-28T10:00:01.000Z",
            "request": ["credential": rawCredential],
            "before": ["snapshotID": "token=\(rawCredential)", "nodeCount": 1],
            "policy": ["decision": "token=\(rawCredential)", "permissions": ["Bearer \(rawCredential)"]],
            "outcome": [
                "success": true,
                "effect": "submitted",
                "verification": ["status": "not_attempted", "strategy": "test"],
                "message": "token=\(rawCredential)",
                "errorCode": "api_key=\(rawCredential)"
            ]
        ]
        let data = try JSONSerialization.data(withJSONObject: envelope, options: [.sortedKeys])

        let decoded = try DiagnosticTraceSerializer().decode(data)

        XCTAssertEqual(decoded.request["credential"], "<redacted>")
        XCTAssertEqual(decoded.before?.snapshotID, "<redacted>")
        XCTAssertEqual(decoded.policy.decision, "<redacted>")
        XCTAssertEqual(decoded.policy.permissions, ["<redacted>"])
        XCTAssertEqual(decoded.outcome.message, "<redacted>")
        XCTAssertEqual(decoded.outcome.errorCode, "<redacted>")
        XCTAssertFalse(String(decoding: data, as: UTF8.self).contains("<redacted>"))
    }
}
