import XCTest
@testable import SymOperateCore
@testable import SymOperateMCP

final class PermissionServiceTests: XCTestCase {
    func testDeniedRequestsFailClosedWithoutOpeningSystemSettings() {
        let probe = FixturePermissionProbe()
        let service = PermissionService(probe: probe)

        XCTAssertFalse(service.requestAccessibilityPermission())
        XCTAssertFalse(service.requestScreenRecordingPermission())
        XCTAssertEqual(probe.accessibilityPrompts, [true, false])
        XCTAssertEqual(probe.screenPreflightCalls, 2)
        XCTAssertEqual(probe.screenRequestCalls, 1)
        XCTAssertEqual(
            probe.openedPrivacyPanes,
            [
                "com.apple.preference.security?Privacy_Accessibility",
                "com.apple.preference.security?Privacy_ScreenCapture",
            ]
        )
    }

    func testStatusUsesFixtureProbeAndDoesNotPrompt() {
        let probe = FixturePermissionProbe(accessibilityGranted: false, screenRecordingGranted: false)
        let service = PermissionService(probe: probe)

        let status = service.status()

        XCTAssertFalse(status.accessibilityGranted)
        XCTAssertFalse(status.screenRecordingGranted)
        XCTAssertEqual(probe.accessibilityPrompts, [false])
        XCTAssertEqual(probe.screenPreflightCalls, 1)
        XCTAssertTrue(probe.openedPrivacyPanes.isEmpty)
    }

    func testPermissionsStatusFixtureFlowsThroughMCPRoute() async throws {
        let probe = FixturePermissionProbe(accessibilityGranted: false, screenRecordingGranted: false)
        let controller = AutomationController(permissions: PermissionService(probe: probe))
        let server = MCPServer(controller: controller)

        let response = try await server.dispatch(method: "tools/call", params: [
            "name": "permissions_status",
            "arguments": [:],
        ])
        let payload = response["structuredContent"] as? [String: Any]

        XCTAssertEqual(response["isError"] as? Bool, false)
        XCTAssertEqual(payload?["accessibilityGranted"] as? Bool, false)
        XCTAssertEqual(payload?["screenRecordingGranted"] as? Bool, false)
        XCTAssertEqual(probe.accessibilityPrompts, [false])
        XCTAssertEqual(probe.screenPreflightCalls, 1)
        XCTAssertTrue(probe.openedPrivacyPanes.isEmpty)
    }
}

private final class FixturePermissionProbe: PermissionProbeAdapter {
    var accessibilityGranted: Bool
    var screenRecordingGranted: Bool
    var accessibilityPrompts: [Bool] = []
    var screenPreflightCalls = 0
    var screenRequestCalls = 0
    var openedPrivacyPanes: [String] = []

    init(accessibilityGranted: Bool = false, screenRecordingGranted: Bool = false) {
        self.accessibilityGranted = accessibilityGranted
        self.screenRecordingGranted = screenRecordingGranted
    }

    func accessibilityTrusted(prompt: Bool) -> Bool {
        accessibilityPrompts.append(prompt)
        return accessibilityGranted
    }

    func screenCapturePreflight() -> Bool {
        screenPreflightCalls += 1
        return screenRecordingGranted
    }

    func requestScreenCapture() -> Bool {
        screenRequestCalls += 1
        return screenRecordingGranted
    }

    func openPrivacyPane(_ path: String) -> Bool {
        openedPrivacyPanes.append(path)
        return false
    }
}
