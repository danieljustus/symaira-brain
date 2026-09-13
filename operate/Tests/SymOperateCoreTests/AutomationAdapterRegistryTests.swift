import XCTest
@testable import SymOperateCore

private struct TestAutomationAdapter: AutomationAdapter {
    let metadata: AutomationAdapterMetadata
}

final class AutomationAdapterRegistryTests: XCTestCase {
    func testStandardRegistryDiscoversAllNativeRoutesWithoutExecutingThem() {
        let registry = AutomationAdapterRegistry.standard
        let target = AutomationTarget(bundleIdentifier: "com.example.editor", appFamily: "editor")

        let discovered = registry.discover(for: target)

        XCTAssertEqual(discovered.map(\.kind), [.appIntents, .shortcuts, .appleScript])
        XCTAssertEqual(Set(discovered.map(\.identifier)), ["app-intents", "shortcuts", "apple-script"])
        XCTAssertTrue(discovered.allSatisfy { $0.isLocal && $0.isDeterministic })
        XCTAssertEqual(discovered.first(where: { $0.kind == .appleScript })?.requiredPermissions, [.automation])
        XCTAssertEqual(discovered.first(where: { $0.kind == .shortcuts })?.requiredPermissions, [.shortcuts])
        XCTAssertTrue(discovered.first(where: { $0.kind == .appIntents })?.requiredPermissions.isEmpty == true)
    }

    func testSelectionUsesSafeOrderingInsteadOfRegistrationOrder() {
        let registry = AutomationAdapterRegistry(adapters: [
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "script-route",
                kind: .appleScript,
                capabilities: [.execute],
                requiredPermissions: [.automation],
                timeout: 10
            )),
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "intent-route",
                kind: .appIntents,
                capabilities: [.execute],
                timeout: 10
            )),
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "shortcut-route",
                kind: .shortcuts,
                capabilities: [.execute],
                requiredPermissions: [.shortcuts],
                timeout: 30
            ))
        ])
        let target = AutomationTarget(bundleIdentifier: "com.example.editor")

        let decision = registry.select(
            capability: .execute,
            for: target,
            grantedPermissions: [.automation, .shortcuts]
        )

        XCTAssertEqual(decision.status, .selected)
        XCTAssertEqual(decision.route, .appIntents)
        XCTAssertEqual(decision.adapterIdentifier, "intent-route")
        XCTAssertEqual(decision.timeout, 10)
        XCTAssertTrue(decision.isExecutable)
    }

    func testSelectionFailsClosedWhenPermissionIsMissing() {
        let registry = AutomationAdapterRegistry.standard
        let target = AutomationTarget(bundleIdentifier: "com.example.editor")

        let decision = registry.select(
            capability: .executeScript,
            for: target,
            grantedPermissions: []
        )

        XCTAssertEqual(decision.status, .refused)
        XCTAssertNil(decision.route)
        XCTAssertNil(decision.adapterIdentifier)
        XCTAssertFalse(decision.isExecutable)
        XCTAssertTrue(decision.reason.contains("automation"))
        XCTAssertEqual(decision.rejections.count, 3)
    }

    func testSelectionRequiresExplicitTargetIdentityAndRejectsMismatches() {
        let registry = AutomationAdapterRegistry.standard

        let missingTarget = registry.select(capability: .runShortcut, for: AutomationTarget())
        XCTAssertEqual(missingTarget.status, .refused)
        XCTAssertTrue(missingTarget.reason.contains("explicit target"))

        let restricted = AutomationAdapterRegistry(adapters: [
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "editor-script",
                kind: .appleScript,
                appFamily: "editor",
                bundleIdentifier: "com.example.editor",
                capabilities: [.executeScript],
                requiredPermissions: [.automation],
                timeout: 10
            ))
        ])
        let mismatch = restricted.select(
            capability: .executeScript,
            for: AutomationTarget(bundleIdentifier: "com.example.other", appFamily: "other"),
            grantedPermissions: [.automation]
        )
        XCTAssertEqual(mismatch.status, .refused)
        XCTAssertTrue(mismatch.reason.contains("target"))
    }

    func testUnsafeMetadataIsNeverSelected() {
        let registry = AutomationAdapterRegistry(adapters: [
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "unbounded",
                kind: .appleScript,
                capabilities: [.executeScript],
                requiredPermissions: [.automation],
                timeout: 0
            )),
            TestAutomationAdapter(metadata: AutomationAdapterMetadata(
                identifier: "remote",
                kind: .shortcuts,
                capabilities: [.runShortcut],
                requiredPermissions: [.shortcuts],
                timeout: 30,
                isLocal: false
            ))
        ])
        let target = AutomationTarget(bundleIdentifier: "com.example.editor")

        let script = registry.select(
            capability: .executeScript,
            for: target,
            grantedPermissions: [.automation]
        )
        let shortcut = registry.select(
            capability: .runShortcut,
            for: target,
            grantedPermissions: [.shortcuts]
        )

        XCTAssertEqual(script.status, .refused)
        XCTAssertEqual(shortcut.status, .refused)
        XCTAssertTrue(script.rejections.contains { $0.reason.contains("timeout") })
        XCTAssertTrue(shortcut.rejections.contains { $0.reason.contains("local") })
    }

    func testMetadataAndDecisionRoundTripAsStructuredData() throws {
        let metadata = AutomationAdapterMetadata(
            identifier: "editor-intent",
            kind: .appIntents,
            appFamily: "editor",
            bundleIdentifier: "com.example.editor",
            capabilities: [.execute, .observe],
            requiredPermissions: [.appIntents],
            timeout: 12
        )
        let selected = AutomationRouteDecision.selected(metadata: metadata)

        let metadataData = try JSONEncoder().encode(metadata)
        let decisionData = try JSONEncoder().encode(selected)

        XCTAssertEqual(try JSONDecoder().decode(AutomationAdapterMetadata.self, from: metadataData), metadata)
        XCTAssertEqual(try JSONDecoder().decode(AutomationRouteDecision.self, from: decisionData), selected)
        XCTAssertEqual(metadata.capabilityNames, ["execute", "observe"])
        XCTAssertEqual(metadata.permissionNames, ["app_intents"])
    }
}
