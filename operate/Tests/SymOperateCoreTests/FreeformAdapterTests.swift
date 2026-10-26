import XCTest
import SQLite3
import SymOperateMCP
@testable import SymOperateCore

private final class FreeformCallLog {
    var calls: [String] = []
}

private final class MockFreeformEnvironment: FreeformEnvironmentProtocol {
    let log: FreeformCallLog
    var pid: Int32 = 7
    var readyAfter = 0
    var polls = 0
    var failAt: String?
    var positions = ["Alpha": PointValue(x: 110, y: 220), "Beta": PointValue(x: 400, y: 400)]
    var firstMoveOffset = 0.0
    var moves = 0
    /// Number of times Freeform re-activates itself right after focus is handed back (observed live).
    var stealBacks = 0
    var windowTitles = ["Test Board"]

    init(_ log: FreeformCallLog) { self.log = log }
    func doctor() -> FreeformDoctor {
        log.calls.append("doctor")
        return FreeformDoctor(running: true, accessibility: true, windowCount: windowTitles.count, processID: 42,
                              windowTitles: windowTitles)
    }
    func frontmostPID() -> Int32? { log.calls.append("frontmost"); return pid }
    func bringToFront(pid: Int32) throws {
        log.calls.append("focus:\(pid)")
        if failAt == "focus:\(pid)" { throw AutomationError.unavailable("focus failed") }
        self.pid = pid
        if pid != 42, log.calls.contains("paste"), stealBacks > 0 { stealBacks -= 1; self.pid = 42 }
    }
    func pasteEnabled() throws -> Bool { log.calls.append("enabled"); polls += 1; return polls > readyAfter }
    func deselectIfAvailable() throws { log.calls.append("deselect") }
    func pressPaste() throws {
        log.calls.append("paste")
        if failAt == "paste" { throw AutomationError.unavailable("paste failed") }
    }
    func pause(seconds: Double) { log.calls.append("wait:\(seconds)") }
    func position(label: String) throws -> PointValue {
        log.calls.append("position:\(label)")
        guard let value = positions[label] else { throw AutomationError.notFound("label") }
        return value
    }
    func selectOnly(label: String) throws { log.calls.append("select:\(label)") }
    func setPosition(label: String, point: PointValue) throws {
        log.calls.append("move:\(label)")
        moves += 1
        positions[label] = PointValue(x: point.x + firstMoveOffset, y: point.y)
    }
}

private final class MockFreeformPasteboard: FreeformPasteboardProtocol {
    let log: FreeformCallLog
    var contents: FreeformPasteboardSnapshot = [["public.utf8-plain-text": Data("original".utf8), "private.binary": Data([0, 255])],
                                                ["public.data": Data([1, 2, 3])]]
    var restored = false
    var restoreOK = true
    var failWrite = false
    init(_ log: FreeformCallLog) { self.log = log }
    func snapshot() throws -> FreeformPasteboardSnapshot { log.calls.append("backup"); return contents }
    func write(_ data: Data, type: String) throws {
        log.calls.append("write")
        contents = [[type: data]]
        if failWrite { throw AutomationError.unavailable("write failed") }
    }
    func restore(_ snapshot: FreeformPasteboardSnapshot) -> Bool {
        log.calls.append("restore")
        contents = snapshot
        restored = true
        return restoreOK
    }
}

private final class MockSnapshotService: FreeformSnapshotServiceProtocol {
    var helperInstalled = true
    var lastStatus: FreeformHelperStatus? = FreeformHelperStatus(ok: false, created_at: 1, error: "FDA denied")
    var calls = 0
    var result: FreeformSnapshotResult?
    func snapshot(timeout: TimeInterval) -> FreeformSnapshotResult {
        calls += 1
        return result ?? FreeformSnapshotResult(ok: false, snapshot_path: nil, created_at: 1, error: "FDA denied")
    }
}

final class FreeformAdapterTests: XCTestCase {
    func testSnapshotMCPValidatesPermissionAndBoardBeforeStartingAndDoctorIsPassive() async throws {
        let log = FreeformCallLog()
        let snapshot = MockSnapshotService()
        let controller = AutomationController(permissions: MockPermissionService(), screen: MockScreenService(),
            apps: MockAppService(), accessibility: MockAccessibilityService(), input: MockInputService())
        let adapter = FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log),
                                      snapshotService: snapshot)
        let server = SymOperateMCP.MCPServer(controller: controller, freeform: adapter)
        let doctor = try await server.dispatch(method: "tools/call", params: ["name": "freeform_doctor"])
        let content = try XCTUnwrap(doctor["structuredContent"] as? [String: Any])
        XCTAssertEqual(content["helper_installed"] as? Bool, true)
        XCTAssertEqual((content["helper_last_status"] as? [String: Any])?["error"] as? String, "FDA denied")
        XCTAssertEqual(snapshot.calls, 0)
        for arguments: [String: Any] in [["read": true], ["read": "true"], ["timeout": true], ["timeout": -1]] {
            do {
                _ = try await server.dispatch(method: "tools/call", params: ["name": "freeform_snapshot", "arguments": arguments])
                XCTFail("Invalid snapshot request must fail before launchd")
            } catch {}
        }
        XCTAssertEqual(snapshot.calls, 0)
        controller.actionPolicy = ActionPolicy(grantedPermissions: [])
        for name in ["freeform_snapshot", "freeform_list_boards", "freeform_read_board"] {
            do {
                _ = try await server.dispatch(method: "tools/call", params: ["name": name])
                XCTFail("Capture permission required")
            } catch let error as AutomationError {
                XCTAssertEqual(error.code, AutomationError.permissionDenied("").code)
            }
        }
        XCTAssertEqual(snapshot.calls, 0)
        controller.actionPolicy = ActionPolicy()
        let failed = try await server.dispatch(method: "tools/call", params: ["name": "freeform_snapshot"])
        XCTAssertEqual((failed["structuredContent"] as? [String: Any])?["ok"] as? Bool, false)
        XCTAssertEqual(snapshot.calls, 1)
        XCTAssertEqual(log.calls, ["doctor"], "No focus, clipboard or GUI writes")
    }

    func testSnapshotMCPReadUsesOnlyFreshSnapshotAndExplicitBoard() async throws {
        let directory = operateTestTemporaryDirectory().appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = directory.appendingPathComponent("boards.db").path
        var database: OpaquePointer?
        XCTAssertEqual(sqlite3_open(path, &database), SQLITE_OK)
        let board = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        let other = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
        XCTAssertEqual(sqlite3_exec(database, """
            CREATE TABLE board_items (item_uuid BLOB, board_identifier BLOB, item_type INT, common_data BLOB, specific_data BLOB, tombstoned INT);
            INSERT INTO board_items VALUES (X'11111111111111111111111111111111',X'\(board)',3,X'',X'',0);
            INSERT INTO board_items VALUES (X'22222222222222222222222222222222',X'\(other)',3,X'',X'',0);
            """, nil, nil, nil), SQLITE_OK)
        sqlite3_close(database)
        let snapshot = MockSnapshotService()
        snapshot.result = FreeformSnapshotResult(ok: true, snapshot_path: path, created_at: 2, error: nil)
        let log = FreeformCallLog()
        let controller = AutomationController(permissions: MockPermissionService(), screen: MockScreenService(),
            apps: MockAppService(), accessibility: MockAccessibilityService(), input: MockInputService())
        let server = SymOperateMCP.MCPServer(controller: controller, freeform: FreeformAdapter(
            environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log), snapshotService: snapshot))
        let result = try await server.dispatch(method: "tools/call", params: ["name": "freeform_snapshot",
            "arguments": ["read": true, "board_identifier": "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"]])
        // Arrays are wrapped by the shared encoder under result; compare with the existing read tool wire shape.
        let direct = try await server.dispatch(method: "tools/call", params: ["name": "freeform_read_board",
            "arguments": ["snapshot_path": path, "board_identifier": "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"]])
        XCTAssertEqual(try JSONSerialization.data(withJSONObject: result, options: .sortedKeys),
                       try JSONSerialization.data(withJSONObject: direct, options: .sortedKeys))
        XCTAssertEqual(snapshot.calls, 1)
        XCTAssertTrue(log.calls.isEmpty)
    }

    func testMenuShortcutMatchingIgnoresLocalizationAndRejectsWrongModifierMasks() {
        for command in ["v", "c", "a"] {
            XCTAssertTrue(FreeformMenuShortcut.matches(character: command.uppercased(), mask: 0,
                                                       command: command, expectedMask: 0))
            for mask in [1, 2, 4, 8, 9] {
                XCTAssertFalse(FreeformMenuShortcut.matches(character: command, mask: mask,
                                                            command: command, expectedMask: 0))
            }
        }
        XCTAssertTrue(FreeformMenuShortcut.matches(character: "A", mask: 1, command: "a", expectedMask: 1))
        XCTAssertFalse(FreeformMenuShortcut.matches(character: nil, mask: nil, command: "v", expectedMask: 0))
    }

    func testPastePipelineOrderAndAllItemsRestored() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        let clipboard = MockFreeformPasteboard(log)
        let original = clipboard.contents
        let result = try FreeformAdapter(environment: env, pasteboard: clipboard).insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertTrue(result.submitted)
        XCTAssertTrue(result.focusRestored)
        XCTAssertTrue(result.pasteboardRestored)
        XCTAssertNil(result.error)
        XCTAssertEqual(result.objects.count, 2)
        XCTAssertEqual(result.connectionCount, 1)
        XCTAssertEqual(result.effect, "unverifiable")
        XCTAssertEqual(clipboard.contents, original)
        XCTAssertEqual(log.calls, ["doctor", "frontmost", "backup", "write", "focus:42", "enabled", "deselect", "paste",
                                  "wait:0.3", "focus:7", "wait:0.25", "frontmost", "restore"])
    }

    func testFocusRestoreIsReassertedWhenFreeformStealsFocusBack() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        env.stealBacks = 1
        let result = try FreeformAdapter(environment: env, pasteboard: MockFreeformPasteboard(log)).insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertTrue(result.focusRestored)
        XCTAssertNil(result.error)
        XCTAssertEqual(Array(log.calls.suffix(7)), ["focus:7", "wait:0.25", "frontmost", "focus:7", "wait:0.25", "frontmost", "restore"])
    }

    func testFocusRestoreReportsFailureWhenPreviousAppNeverStaysFrontmost() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        env.stealBacks = 99
        let result = try FreeformAdapter(environment: env, pasteboard: MockFreeformPasteboard(log)).insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertTrue(result.submitted)
        XCTAssertFalse(result.focusRestored)
        XCTAssertTrue(result.pasteboardRestored)
        XCTAssertEqual(log.calls.filter { $0 == "focus:7" }.count, 3)
        XCTAssertTrue(result.error?.contains("Focus restoration") == true)
    }

    func testPollingAndTimeoutStillRestoreFocusAndClipboard() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        env.readyAfter = 100
        let clipboard = MockFreeformPasteboard(log)
        let result = try FreeformAdapter(environment: env, pasteboard: clipboard).insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertFalse(result.submitted)
        XCTAssertTrue(result.focusRestored)
        XCTAssertTrue(result.pasteboardRestored)
        XCTAssertEqual(env.polls, 40)
        XCTAssertFalse(log.calls.contains("paste"))
        XCTAssertNotNil(result.error)
        XCTAssertEqual(Array(log.calls.suffix(4)), ["focus:7", "wait:0.25", "frontmost", "restore"])
    }

    func testEveryFailurePathRestoresClipboardEvenIfFocusFails() throws {
        for failure in ["paste", "focus:42", "focus:7", "write"] {
            let log = FreeformCallLog()
            let env = MockFreeformEnvironment(log)
            env.failAt = failure
            let clipboard = MockFreeformPasteboard(log)
            clipboard.failWrite = failure == "write"
            let result = try FreeformAdapter(environment: env, pasteboard: clipboard).insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
            XCTAssertTrue(clipboard.restored, failure)
            XCTAssertTrue(result.pasteboardRestored, failure)
            XCTAssertNotNil(result.error, failure)
            if failure == "focus:7" { XCTAssertFalse(result.focusRestored) }
        }
    }

    func testRestoreFailureIsReportedNotHidden() throws {
        let log = FreeformCallLog()
        let clipboard = MockFreeformPasteboard(log)
        clipboard.restoreOK = false
        let result = try FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: clipboard)
            .insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertFalse(result.pasteboardRestored)
    }

    func testInvalidArrowNeverTouchesSystem() {
        let log = FreeformCallLog()
        var board = freeformTestBoard()
        board.connections = [FreeformConnection(tail: board.objects[0].id, head: board.objects[1].id, lineEnd: "arrow")]
        XCTAssertThrowsError(try FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log))
            .insert(board, boardTitle: "Test Board", policy: ActionPolicy()))
        XCTAssertEqual(log.calls, [])
    }

    func testEachWriteGrantIsRequiredBeforeAnySystemCall() {
        for removed in [PermissionFlags.input, .appControl, .menuAction] {
            let log = FreeformCallLog()
            let adapter = FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log))
            let policy = ActionPolicy(grantedPermissions: PermissionFlags.all.subtracting(removed))
            XCTAssertThrowsError(try adapter.insert(freeformTestBoard(), boardTitle: "Test Board", policy: policy))
            XCTAssertFalse(adapter.plan(policy: policy, name: "insert").executable)
            XCTAssertEqual(log.calls, [])
        }
    }

    func testScopedPolicyAppliesEquallyToPlanAndExecution() throws {
        let log = FreeformCallLog()
        let adapter = FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log))
        let scoped = ActionPolicy(scopedGrants: [ScopedPermissionGrant(scope: PolicyScope(application: FreeformDescriptor.bundleID),
                                                                     permissions: FreeformAdapter.writePermissions)])
        XCTAssertTrue(adapter.plan(policy: scoped, name: "insert").executable)
        XCTAssertTrue(try adapter.insert(freeformTestBoard(), boardTitle: "Test Board", policy: scoped).submitted)
    }

    func testMoveUsesReferenceTransformAndSingleSelectionWithoutFocusOrPasteboard() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        let adapter = FreeformAdapter(environment: env, pasteboard: MockFreeformPasteboard(log))
        let result = try adapter.move(FreeformMoveRequest(label: "Beta", referenceLabel: "Alpha", referenceX: 10,
                                                        referenceY: 20, x: 250, y: 350), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertTrue(result.confirmed)
        XCTAssertFalse(result.corrected)
        XCTAssertEqual(result.screenPosition, PointValue(x: 350, y: 550))
        XCTAssertEqual(result.boardPosition, PointValue(x: 250, y: 350))
        XCTAssertEqual(log.calls, ["doctor", "position:Alpha", "deselect", "select:Beta", "move:Beta", "wait:0.3", "position:Beta"])
        XCTAssertEqual(env.pid, 7)
    }

    func testMoveCorrectionAtHalfPoint() throws {
        let log = FreeformCallLog()
        let env = MockFreeformEnvironment(log)
        env.firstMoveOffset = 0.5
        let result = try FreeformAdapter(environment: env, pasteboard: MockFreeformPasteboard(log))
            .move(FreeformMoveRequest(label: "Beta", referenceLabel: "Alpha", referenceX: 10,
                                      referenceY: 20, x: 250, y: 350), boardTitle: "Test Board", policy: ActionPolicy())
        XCTAssertTrue(result.corrected)
        XCTAssertTrue(result.confirmed)
        XCTAssertEqual(env.moves, 2)
    }

    func testRegistryAndDescriptorAreVersionedAndPermissionGated() {
        let target = AutomationTarget(bundleIdentifier: FreeformDescriptor.bundleID, appFamily: "freeform")
        let registry = AutomationAdapterRegistry.standard
        XCTAssertEqual(registry.discover(for: target).last?.identifier, "freeform")
        XCTAssertEqual(registry.select(capability: .execute, for: target).status, .refused)
        XCTAssertEqual(registry.select(capability: .execute, for: target, grantedPermissions: [.accessibility]).adapterIdentifier, "freeform")
        let descriptor = FreeformDescriptor()
        XCTAssertEqual(descriptor.version, 1)
        XCTAssertEqual(descriptor.capabilityProfile.appFamily, "freeform")
        XCTAssertEqual(descriptor.capabilityProfile.capabilities, ["insert", "move", "connect", "read_snapshot"])
    }

    func testMCPFreeformCatalogAndDryRunUseNoRealServices() async throws {
        let log = FreeformCallLog()
        let controller = AutomationController(permissions: MockPermissionService(), screen: MockScreenService(),
                                              apps: MockAppService(), accessibility: MockAccessibilityService(), input: MockInputService())
        let server = SymOperateMCP.MCPServer(controller: controller,
                    freeform: FreeformAdapter(environment: MockFreeformEnvironment(log), pasteboard: MockFreeformPasteboard(log)))
        let catalog = try await server.dispatch(method: "tools/list", params: [:])["tools"] as! [[String: Any]]
        XCTAssertEqual(Set(catalog.compactMap { $0["name"] as? String }.filter { $0.hasPrefix("freeform_") }),
                       ["freeform_doctor", "freeform_insert", "freeform_move", "freeform_connect", "freeform_read_board", "freeform_list_boards", "freeform_snapshot"])
        let board = try JSONSerialization.jsonObject(with: JSONEncoder().encode(freeformTestBoard()))
        for tool in ["freeform_insert", "freeform_connect"] {
            let result = try await server.dispatch(method: "tools/call", params: ["name": tool, "arguments": ["board": board, "board_title": "Test Board", "dry_run": true]])
            XCTAssertEqual((result["structuredContent"] as? [String: Any])?["executable"] as? Bool, true)
        }
        XCTAssertEqual(log.calls, [])
        controller.actionPolicy = ActionPolicy(grantedPermissions: [])
        do {
            _ = try await server.dispatch(method: "tools/call", params: ["name": "freeform_insert", "arguments": ["board": board, "board_title": "Test Board"]])
            XCTFail("Missing permission should refuse")
        } catch let error as AutomationError { XCTAssertEqual(error.code, AutomationError.permissionDenied("").code) }
        XCTAssertEqual(log.calls, [])
        controller.actionPolicy = ActionPolicy()
        do {
            _ = try await server.dispatch(method: "tools/call", params: ["name": "freeform_insert", "arguments": ["board": board]])
            XCTFail("Missing board_title should refuse")
        } catch let error as AutomationError { XCTAssertEqual(error.code, AutomationError.invalidArgument("").code) }
        XCTAssertEqual(log.calls, [])
    }

    func testWritesFailClosedUnlessExactlyTheNamedBoardIsOpen() throws {
        for titles in [["Prozessmanagement"], ["Test Board", "Other"], [], ["test board"]] {
            let log = FreeformCallLog()
            let env = MockFreeformEnvironment(log)
            env.windowTitles = titles
            let adapter = FreeformAdapter(environment: env, pasteboard: MockFreeformPasteboard(log))
            XCTAssertThrowsError(try adapter.insert(freeformTestBoard(), boardTitle: "Test Board", policy: ActionPolicy()))
            XCTAssertThrowsError(try adapter.move(FreeformMoveRequest(label: "Beta", referenceLabel: "Alpha", referenceX: 10,
                                                                      referenceY: 20, x: 250, y: 350),
                                                  boardTitle: "Test Board", policy: ActionPolicy()))
            XCTAssertEqual(log.calls, ["doctor", "doctor"], "no pasteboard, focus or AX write for \(titles)")
        }
    }
}
