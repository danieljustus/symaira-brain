import Foundation
import XCTest
@testable import SymOperateCore
@testable import SymOperateMCP

final class StartupPolicyTests: XCTestCase {
    private func temporaryPolicy(_ contents: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-policy-\(UUID().uuidString).json")
        try contents.data(using: .utf8)!.write(to: url)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    func testNoStartupSourcesKeepFullGrantCompatibility() throws {
        let missing = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-missing-\(UUID().uuidString).json")
        let policy = try StartupPolicy.load(grantNames: nil, policyURL: missing)
        XCTAssertEqual(policy.grantedPermissions, .all)
        XCTAssertEqual(policy.startupGrantedPermissions, .all)
    }

    func testPolicyFileLoadsGrantedPermissions() throws {
        let url = try temporaryPolicy(#"{"granted_permissions":["capture","input"]}"#)
        let policy = try StartupPolicy.load(grantNames: nil, policyURL: url)
        XCTAssertEqual(policy.grantedPermissions.flagNames, ["capture", "input"])
        XCTAssertEqual(policy.startupGrantedPermissions.flagNames, ["capture", "input"])
    }

    func testExplicitGrantWinsWithoutReadingPolicyFile() throws {
        let url = try temporaryPolicy("not json")
        let policy = try StartupPolicy.load(grantNames: ["capture"], policyURL: url)
        XCTAssertEqual(policy.grantedPermissions.flagNames, ["capture"])
    }

    func testSetGrantedPermissionsCannotWidenStartupGrant() {
        var policy = ActionPolicy(
            grantedPermissions: [.capture, .policyModify],
            startupGrantedPermissions: [.capture, .policyModify]
        )
        XCTAssertTrue(policy.setGrantedPermissions([.capture]))
        XCTAssertFalse(policy.setGrantedPermissions([.capture, .input]))
        XCTAssertEqual(policy.grantedPermissions, [.capture])
    }

    func testMCPSetPolicyWideningIsClassifiedAndDoesNotPartiallyMutate() async throws {
        let controller = AutomationController(
            actionPolicy: ActionPolicy(
                grantedPermissions: [.capture, .policyModify],
                startupGrantedPermissions: [.capture, .policyModify]
            ),
            history: HistoryService(fileURL: FileManager.default.temporaryDirectory
                .appendingPathComponent("symoperate-policy-mcp-\(UUID().uuidString).jsonl"))
        )
        let server = MCPServer(controller: controller)

        do {
            _ = try await server.dispatch(method: "tools/call", params: [
                "name": "set_policy",
                "arguments": [
                    "granted_permissions": ["capture", "input"],
                    "allow_keywords": ["save"],
                ],
            ])
            XCTFail("Expected widening startup grant to be refused")
        } catch let error as AutomationError {
            guard case .permissionDenied(let message) = error else {
                return XCTFail("Expected classified permissionDenied, got \(error)")
            }
            XCTAssertTrue(message.contains("startup grant"))
        }

        let result = try await server.dispatch(method: "tools/call", params: [
            "name": "get_policy", "arguments": [:],
        ])
        let policy = result["structuredContent"] as? [String: Any]
        XCTAssertEqual(policy?["granted_permissions"] as? [String], ["capture", "policy_modify"])
        XCTAssertEqual(policy?["allowedKeywords"] as? [String], [])
    }

    func testScopedGrantsIntersectStartupAndEachApplicableDimension() {
        let policy = ActionPolicy(
            grantedPermissions: [.capture, .input, .appControl],
            startupGrantedPermissions: [.capture, .input],
            scopedGrants: [
                ScopedGrant(scope: PolicyScope(agent: "agent-a"), permissions: [.capture, .input]),
                ScopedGrant(scope: PolicyScope(application: "com.example.Editor"), permissions: [.capture]),
                ScopedGrant(scope: PolicyScope(workflow: "review"), permissions: [.capture, .input]),
            ]
        )

        XCTAssertEqual(
            policy.effectivePermissions(for: PolicyScope(agent: "agent-a", application: "com.example.Editor", workflow: "review")),
            [.capture]
        )
    }

    func testScopedGrantMissingOrMismatchedIdentityFailsClosed() {
        let policy = ActionPolicy(
            grantedPermissions: .all,
            startupGrantedPermissions: .all,
            scopedGrants: [
                ScopedGrant(scope: PolicyScope(agent: "agent-a"), permissions: [.capture]),
                ScopedGrant(scope: PolicyScope(application: "com.example.Editor"), permissions: [.capture]),
                ScopedGrant(scope: PolicyScope(workflow: "review"), permissions: [.capture]),
            ]
        )

        XCTAssertEqual(policy.effectivePermissions(for: PolicyScope(agent: "agent-a", application: "com.example.Other", workflow: "review")), [])
        XCTAssertEqual(policy.effectivePermissions(), [])
    }

    func testPolicyFileParsesScopedGrantRecords() throws {
        let url = try temporaryPolicy(#"{"grant":["capture","input"],"scoped_grants":[{"agent":"agent-a","application":"com.example.Editor","workflow":"review","granted_permissions":["capture"]}]}"#)
        let policy = try StartupPolicy.load(grantNames: nil, policyURL: url)

        XCTAssertEqual(policy.scopedGrants.count, 1)
        XCTAssertEqual(
            policy.effectivePermissions(for: PolicyScope(agent: "agent-a", application: "com.example.Editor", workflow: "review")),
            [.capture]
        )
    }

    func testDoctorReportIncludesEffectiveGrant() throws {
        let report = DoctorReport(
            ok: true,
            version: "test",
            permissions: PermissionSnapshot(
                accessibilityGranted: true,
                screenRecordingGranted: true,
                source: sharedTestPermissionSource
            ),
            capabilities: [:],
            environment: EnvironmentReport(
                platform: "macOS", macOSVersion: "test", swiftVersion: "test",
                appsCount: 0, displaysCount: 0
            ),
            recommendations: [],
            effectiveGrant: ["capture"]
        )
        let data = try JSONEncoder().encode(report)
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        XCTAssertEqual(object?["effective_grant"] as? [String], ["capture"])
    }
}
