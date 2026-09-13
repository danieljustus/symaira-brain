import XCTest
@testable import SymOperateCore

private actor WorkflowCallLog {
    private(set) var stepIDs: [String] = []
    private(set) var actions: [String] = []

    func append(stepID: String) {
        stepIDs.append(stepID)
    }

    func append(action: String) {
        actions.append(action)
    }
}

private final class WorkflowHistoryBox: @unchecked Sendable {
    private let lock = NSLock()
    private var storedEvents: [WorkflowHistoryEvent] = []

    func append(_ event: WorkflowHistoryEvent) {
        lock.lock()
        storedEvents.append(event)
        lock.unlock()
    }

    var events: [WorkflowHistoryEvent] {
        lock.lock()
        defer { lock.unlock() }
        return storedEvents
    }
}

private func confirmedWorkflowResult() -> ActionResult {
    ActionResult(
        ok: true,
        message: "confirmed",
        effect: .confirmed,
        verification: ActionVerification(
            status: .confirmed,
            strategy: "test",
            checkedAt: nil
        )
    )
}

private func failedWorkflowResult() -> ActionResult {
    ActionResult(
        ok: false,
        message: "failed",
        effect: .refused,
        verification: ActionVerification(
            status: .notAttempted,
            strategy: "test",
            checkedAt: nil
        )
    )
}

final class WorkflowRunnerTests: XCTestCase {
    private let condition = ActionConditions(
        postcondition: UIElementPredicate(label: "Ready")
    )

    func testConfirmedStepsRunInOrderAndAggregateDeterministically() async {
        let log = WorkflowCallLog()
        let condition = self.condition
        let plan = WorkflowPlan(id: "ordered", steps: [
            WorkflowStep(id: "first", action: "click", conditions: condition),
            WorkflowStep(id: "second", action: "wait_for", conditions: condition),
            WorkflowStep(id: "third", action: "type_text", conditions: condition),
        ])
        let runner = WorkflowRunner(executor: { step in
            await log.append(stepID: step.id)
            return confirmedWorkflowResult()
        })

        let result = await runner.run(plan)

        let calls = await log.stepIDs
        XCTAssertEqual(calls, ["first", "second", "third"])
        XCTAssertEqual(result.status, .completed)
        XCTAssertEqual(result.steps.map(\.stepID), ["first", "second", "third"])
        XCTAssertTrue(result.steps.allSatisfy { $0.status == .completed })
        XCTAssertEqual(result.completedStepCount, 3)
        XCTAssertTrue(result.compensations.isEmpty)
    }

    func testUnverifiableStepAbortsAndSkipsRemainingSteps() async {
        let log = WorkflowCallLog()
        let condition = self.condition
        let plan = WorkflowPlan(id: "abort", steps: [
            WorkflowStep(id: "before", action: "click", conditions: condition),
            WorkflowStep(id: "unsafe", action: "type_text", conditions: condition),
            WorkflowStep(id: "after", action: "press_keys", conditions: condition),
        ])
        let runner = WorkflowRunner(executor: { step in
            await log.append(stepID: step.id)
            if step.id == "unsafe" {
                return ActionResult(
                    ok: true,
                    message: "submitted",
                    effect: .unverifiable,
                    verification: ActionVerification(status: .unverifiable, strategy: "test", checkedAt: nil)
                )
            }
            return confirmedWorkflowResult()
        })

        let result = await runner.run(plan)

        let calls = await log.stepIDs
        XCTAssertEqual(calls, ["before", "unsafe"])
        XCTAssertEqual(result.status, .failed)
        XCTAssertEqual(result.failureStepID, "unsafe")
        XCTAssertEqual(result.steps.map(\.status), [.completed, .failed, .skipped])
        XCTAssertEqual(result.compensations.count, 1)
        XCTAssertEqual(result.compensations[0].status, .unavailable)
        XCTAssertEqual(result.compensations[0].stepID, "before")
    }

    func testMaximumStepBoundAndHistoryHookRedactSensitiveParameters() async {
        let history = WorkflowHistoryBox()
        let condition = self.condition
        let steps = (0..<WorkflowPlan.maxSteps + 1).map { index in
            WorkflowStep(
                id: "step-\(index)",
                action: "type_text",
                parameters: ["text": "my-secret-password"],
                conditions: condition
            )
        }
        let plan = WorkflowPlan(id: "bounded", steps: steps)
        let runner = WorkflowRunner(
            executor: { _ in XCTFail("An over-bound plan must not execute"); return confirmedWorkflowResult() },
            history: { event in history.append(event) }
        )

        let result = await runner.run(plan)

        XCTAssertEqual(result.status, .invalid)
        XCTAssertEqual(result.steps.count, WorkflowPlan.maxSteps + 1)
        XCTAssertTrue(result.steps.allSatisfy { $0.status == .skipped })
        let events = history.events
        XCTAssertEqual(events.count, WorkflowPlan.maxSteps + 1)
        XCTAssertEqual(events[0].parameters["text"], "<redacted>")
        XCTAssertFalse(events[0].parameters.values.contains("my-secret-password"))
    }

    func testCompensatesCompletedReversibleStepsInReverseOrder() async {
        let log = WorkflowCallLog()
        let condition = self.condition
        let recoverFirst = WorkflowRecovery.reversible(
            WorkflowCompensation(action: "undo_first", conditions: condition)
        )
        let recoverSecond = WorkflowRecovery.reversible(
            WorkflowCompensation(action: "undo_second", conditions: condition)
        )
        let plan = WorkflowPlan(id: "recover", steps: [
            WorkflowStep(id: "first", action: "make_first", conditions: condition, recovery: recoverFirst),
            WorkflowStep(id: "second", action: "make_second", conditions: condition, recovery: recoverSecond),
            WorkflowStep(id: "third", action: "make_third", conditions: condition),
        ])
        let runner = WorkflowRunner(
            executor: { step in
                await log.append(action: "execute:\(step.action)")
                return step.id == "third" ? failedWorkflowResult() : confirmedWorkflowResult()
            },
            compensationExecutor: { compensation in
                await log.append(action: "compensate:\(compensation.action)")
                return confirmedWorkflowResult()
            }
        )

        let result = await runner.run(plan)

        let actions = await log.actions
        XCTAssertEqual(actions, [
            "execute:make_first",
            "execute:make_second",
            "execute:make_third",
            "compensate:undo_second",
            "compensate:undo_first",
        ])
        XCTAssertEqual(result.compensations.map(\.stepID), ["second", "first"])
        XCTAssertTrue(result.compensations.allSatisfy { $0.status == .completed })
    }

    func testUnavailableRecoveryIsReportedWithoutGuessingOrExecuting() async {
        let log = WorkflowCallLog()
        let history = WorkflowHistoryBox()
        let condition = self.condition
        let plan = WorkflowPlan(id: "unsupported", steps: [
            WorkflowStep(id: "first", action: "make_first", conditions: condition),
            WorkflowStep(id: "second", action: "fail", conditions: condition),
        ])
        let runner = WorkflowRunner(
            executor: { step in
                await log.append(action: "execute:\(step.action)")
                return step.id == "second" ? failedWorkflowResult() : confirmedWorkflowResult()
            },
            compensationExecutor: { compensation in
                await log.append(action: "compensate:\(compensation.action)")
                return confirmedWorkflowResult()
            },
            history: { event in history.append(event) }
        )

        let result = await runner.run(plan)

        let actions = await log.actions
        XCTAssertEqual(actions, ["execute:make_first", "execute:fail"])
        XCTAssertEqual(result.compensations.count, 1)
        XCTAssertEqual(result.compensations[0].status, .unavailable)
        XCTAssertEqual(result.compensations[0].stepID, "first")
        XCTAssertTrue(history.events.contains {
            $0.phase == .compensation && $0.action == "recovery_unavailable" && $0.originalStepID == "first"
        })
    }

    func testRecoveryIsSeparatelyPolicyCheckedAndHistoryIsRedacted() async {
        let history = WorkflowHistoryBox()
        let condition = self.condition
        let secret = "undo-secret-value"
        let plan = WorkflowPlan(id: "refused", steps: [
            WorkflowStep(
                id: "first",
                action: "type_text",
                parameters: ["text": "forward-secret-value"],
                conditions: condition,
                recovery: .reversible(WorkflowCompensation(
                    action: "undo_text",
                    parameters: ["text": secret],
                    conditions: condition
                ))
            ),
            WorkflowStep(id: "second", action: "fail", conditions: condition),
        ])
        let runner = WorkflowRunner(
            executor: { step in
                step.id == "second" ? failedWorkflowResult() : confirmedWorkflowResult()
            },
            compensationExecutor: { _ in
                XCTFail("A policy-refused recovery must not execute")
                return confirmedWorkflowResult()
            },
            recoveryPolicy: { _ in false },
            history: { event in history.append(event) }
        )

        let result = await runner.run(plan)

        XCTAssertEqual(result.compensations.count, 1)
        XCTAssertEqual(result.compensations[0].status, .refused)
        guard let compensationEvent = history.events.first(where: {
            $0.phase == .compensation && $0.originalStepID == "first"
        }) else {
            XCTFail("Expected a compensation history event")
            return
        }
        XCTAssertEqual(compensationEvent.parameters["text"], "<redacted>")
        XCTAssertFalse(history.events.contains { $0.parameters.values.contains(secret) })
        XCTAssertFalse(history.events.contains { $0.parameters.values.contains("forward-secret-value") })
    }
}
