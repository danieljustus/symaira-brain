import XCTest
@testable import SymOperateCore
@testable import SymOperateMCP

final class EffectVerificationTests: XCTestCase {
    private func controller(apps: MockAppService = MockAppService()) -> AutomationController {
        AutomationController(
            permissions: MockPermissionService(),
            screen: MockScreenService(),
            apps: apps,
            accessibility: MockAccessibilityService(),
            input: MockInputService(),
            ocr: MockOCRService(),
            queryService: MockUIQueryService(),
            history: HistoryService(fileURL: FileManager.default.temporaryDirectory
                .appendingPathComponent("symoperate-effect-\(UUID().uuidString).jsonl"))
        )
    }

    func testLegacyActionResultDecodesWithConservativeEffectClassification() throws {
        let data = Data(#"{"ok":true,"message":"Click event posted."}"#.utf8)
        let result = try JSONDecoder().decode(ActionResult.self, from: data)

        XCTAssertEqual(result.ok, true)
        XCTAssertEqual(result.effect, .submitted)
        XCTAssertEqual(result.contractVersion, 0)
        XCTAssertEqual(result.verification.status, .unverifiable)
        XCTAssertEqual(result.verification.strategy, "legacy_result")
    }

    func testPostedInputIsSubmittedButNotConfirmedAndHistoryCarriesVerification() throws {
        let historyURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-effect-history-\(UUID().uuidString).jsonl")
        let history = HistoryService(fileURL: historyURL)
        let controller = AutomationController(
            permissions: MockPermissionService(),
            screen: MockScreenService(),
            apps: MockAppService(),
            accessibility: MockAccessibilityService(),
            input: MockInputService(),
            ocr: MockOCRService(),
            queryService: MockUIQueryService(),
            history: history
        )

        let result = try controller.typeText("hello")
        XCTAssertTrue(result.ok)
        XCTAssertEqual(result.effect, .submitted)
        XCTAssertEqual(result.verification.status, .unverifiable)
        XCTAssertEqual(result.executionPath, "cg_event")
        XCTAssertNotEqual(result.effect, .confirmed)

        let events = try history.events()
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(events[0].effect, .submitted)
        XCTAssertEqual(events[0].verification?.status, .unverifiable)
        XCTAssertEqual(events[0].executionPath, "cg_event")
    }

    func testFocusWindowReadbackConfirmsMatchingApplicationAndWindow() throws {
        let apps = MockAppService()
        apps.apps = [AppInfo(localizedName: "Editor", bundleIdentifier: "com.example.editor", processIdentifier: 42, isActive: true)]
        apps.windows = [WindowInfo(
            windowID: 7,
            ownerName: "Editor",
            ownerPID: 42,
            title: "Document",
            bounds: RectValue(x: 0, y: 0, width: 800, height: 600),
            layer: 0
        )]

        let result = try controller(apps: apps).focusWindow(
            bundleID: "com.example.editor",
            appName: nil,
            title: "Document"
        )

        XCTAssertTrue(result.ok)
        XCTAssertEqual(result.effect, .confirmed)
        XCTAssertEqual(result.verification.status, .confirmed)
        XCTAssertEqual(result.verification.strategy, "focus_readback")
        XCTAssertEqual(result.target?.frontmostPID, 42)
        XCTAssertEqual(result.target?.frontmostWindowID, 7)
    }

    func testFocusWindowReadbackClassifiesWrongFrontmostApplicationAsSuspectedNoop() throws {
        let apps = MockAppService()
        apps.apps = [
            AppInfo(localizedName: "Requested", bundleIdentifier: "com.example.requested", processIdentifier: 42, isActive: false),
            AppInfo(localizedName: "Other", bundleIdentifier: "com.example.other", processIdentifier: 99, isActive: true),
        ]

        let result = try controller(apps: apps).focusWindow(
            bundleID: "com.example.requested",
            appName: nil,
            title: nil
        )

        XCTAssertFalse(result.ok)
        XCTAssertEqual(result.effect, .suspectedNoop)
        XCTAssertEqual(result.verification.status, .suspectedNoop)
        XCTAssertEqual(result.target?.frontmostPID, 99)
    }

    func testMCPToolsDocumentVersionedActionResultSchema() async throws {
        let server = SymOperateMCP.MCPServer()
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = try XCTUnwrap(result["tools"] as? [[String: Any]])
        let click = try XCTUnwrap(tools.first { $0["name"] as? String == "click" })
        let schema = try XCTUnwrap(click["outputSchema"] as? [String: Any])
        let required = try XCTUnwrap(schema["required"] as? [String])
        XCTAssertTrue(required.contains("contractVersion"))
        XCTAssertTrue(required.contains("effect"))
        XCTAssertTrue(required.contains("verification"))
        let properties = try XCTUnwrap(schema["properties"] as? [String: Any])
        XCTAssertNotNil(properties["executionPath"])
        XCTAssertNotNil(properties["target"])
        let effect = try XCTUnwrap(properties["effect"] as? [String: Any])
        XCTAssertEqual(effect["enum"] as? [String], ["submitted", "confirmed", "unverifiable", "suspected_noop", "refused"])
    }

    func testEveryEffectStateAndVersionRoundTrip() throws {
        for state in [EffectState.submitted, .confirmed, .unverifiable, .suspectedNoop, .refused] {
            let result = ActionResult(
                ok: state != .refused,
                message: state.rawValue,
                contractVersion: EffectContract.currentVersion,
                effect: state,
                verification: ActionVerification(status: state == .refused ? .notAttempted : .unverifiable, strategy: "test")
            )
            let decoded = try JSONDecoder().decode(ActionResult.self, from: JSONEncoder().encode(result))
            XCTAssertEqual(decoded.contractVersion, EffectContract.currentVersion)
            XCTAssertEqual(decoded.effect, state)
        }
    }

    func testHistoryServiceRoundTripsDiagnosticTrace() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-trace-history-\(UUID().uuidString).jsonl")
        let history = HistoryService(fileURL: url)
        let trace = DiagnosticTraceRecord(
            traceID: "trace-test",
            action: "click",
            startedAt: "2026-08-28T10:00:00Z",
            finishedAt: "2026-08-28T10:00:01Z",
            request: ["label": "Save"],
            target: ActionTarget(requestedAppName: "TextEdit"),
            route: ActionRouteDiagnostics(route: .semantic),
            policy: DiagnosticTracePolicy(decision: "allowed"),
            outcome: DiagnosticTraceOutcome(
                success: true,
                effect: .submitted,
                verification: ActionVerification(status: .unverifiable, strategy: "test"),
                message: "submitted"
            )
        )
        try history.record(HistoryEvent(action: "click", success: true, message: "submitted", diagnosticTrace: trace))

        let events = try history.events()
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(events.first?.diagnosticTrace?.traceID, "trace-test")
    }
}
