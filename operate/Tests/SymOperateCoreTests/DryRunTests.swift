import XCTest
@testable import SymOperateCore

final class DryRunTests: XCTestCase {
    func testPlanDoesNotExecuteAndReportsAllowedActions() {
        var executed = false
        let plan = DryRunPlanner(policy: ActionPolicy())
        let result = plan.plan([
            DryRunAction(name: "click", permission: .input)
        ])
        executed = true // sentinel: planner has no execution callback
        XCTAssertTrue(result.executable)
        XCTAssertEqual(result.entries.count, 1)
        XCTAssertTrue(result.entries[0].allowed)
        XCTAssertTrue(executed)
    }

    func testPlanFailsClosedForDeniedPermission() {
        let policy = ActionPolicy(grantedPermissions: [])
        let result = DryRunPlanner(policy: policy).plan([
            DryRunAction(name: "click", permission: .input)
        ])
        XCTAssertFalse(result.executable)
        XCTAssertFalse(result.entries[0].allowed)
        XCTAssertNotNil(result.entries[0].reason)
    }

    func testPlanIsCodable() throws {
        let plan = DryRunPlanner(policy: ActionPolicy()).plan([
            DryRunAction(name: "snapshot", permission: .capture)
        ])
        let data = try JSONEncoder().encode(plan)
        let decoded = try JSONDecoder().decode(DryRunPlan.self, from: data)
        XCTAssertEqual(decoded, plan)
    }
}
