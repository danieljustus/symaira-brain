import XCTest
@testable import SymOperateCore

private actor WorkflowCancellationCallLog {
    private(set) var stepIDs: [String] = []

    func append(_ stepID: String) {
        stepIDs.append(stepID)
    }
}

private func confirmedWorkflowAction() -> ActionResult {
    ActionResult(
        ok: true,
        message: "confirmed",
        effect: .confirmed,
        verification: ActionVerification(status: .confirmed, strategy: "test", checkedAt: nil)
    )
}

final class WorkflowCancellationTests: XCTestCase {
    private let condition = ActionConditions(postcondition: UIElementPredicate(label: "Ready"))

    private func plan() -> WorkflowPlan {
        WorkflowPlan(id: "bounded-run", steps: [
            WorkflowStep(id: "first", action: "click", conditions: condition),
            WorkflowStep(id: "second", action: "type_text", conditions: condition),
            WorkflowStep(id: "third", action: "press_keys", conditions: condition),
        ])
    }

    func testTimeoutStopsBeforeNextSideEffectAndPersistsTimedOutCheckpoint() async throws {
        let log = WorkflowCancellationCallLog()
        let store = InMemoryWorkflowRunCheckpointStore()
        let runner = DeadlineAwareWorkflowRunner(executor: { step, deadline in
            await log.append(step.id)
            if step.id == "first" {
                try await deadline.sleep(for: 1)
            }
            return confirmedWorkflowAction()
        })

        let result = await runner.run(plan(), timeoutSeconds: 0.03, runID: "timeout", checkpointStore: store)

        let calls = await log.stepIDs
        XCTAssertEqual(result.status, .timedOut)
        XCTAssertTrue(result.failClosed)
        XCTAssertEqual(calls, ["first"])
        let checkpoint = try XCTUnwrap(try store.load(runID: "timeout"))
        XCTAssertEqual(checkpoint.state, .timedOut)
        XCTAssertEqual(checkpoint.currentStepID, "first")
    }

    func testCancellationStopsCurrentRunBeforeStartingAnotherStep() async throws {
        let log = WorkflowCancellationCallLog()
        let runner = DeadlineAwareWorkflowRunner(executor: { step, _ in
            await log.append(step.id)
            try await Task.sleep(nanoseconds: 5_000_000_000)
            return confirmedWorkflowAction()
        })
        let workflowPlan = plan()
        let task = Task {
            await runner.run(workflowPlan, timeoutSeconds: 10, runID: "cancelled")
        }

        try await Task.sleep(nanoseconds: 30_000_000)
        task.cancel()
        let result = await task.value

        let calls = await log.stepIDs
        XCTAssertEqual(result.status, .cancelled)
        XCTAssertTrue(result.failClosed)
        XCTAssertEqual(calls, ["first"])
    }

    func testRestartMarksRunningCheckpointInterruptedAndDoesNotResumeExecutor() async throws {
        let store = InMemoryWorkflowRunCheckpointStore()
        try store.save(WorkflowRunCheckpoint(
            runID: "crashed",
            planID: plan().id,
            nextStepIndex: 1,
            completedStepIDs: ["first"],
            currentStepID: "second",
            state: .running
        ))
        let recovery = try WorkflowRunRecovery.recoverIncompleteRuns(from: store)
        XCTAssertEqual(recovery.count, 1)
        XCTAssertEqual(recovery[0].state, .interrupted)

        let calls = WorkflowCancellationCallLog()
        let runner = DeadlineAwareWorkflowRunner(executor: { step, _ in
            await calls.append(step.id)
            return confirmedWorkflowAction()
        })
        let result = await runner.run(plan(), timeoutSeconds: 10, runID: "crashed", checkpointStore: store)

        let executed = await calls.stepIDs
        XCTAssertEqual(result.status, .interrupted)
        XCTAssertTrue(result.failClosed)
        XCTAssertTrue(executed.isEmpty)
        XCTAssertEqual(try store.load(runID: "crashed")?.state, .interrupted)

        let encoded = try JSONEncoder().encode(recovery[0])
        let decoded = try JSONDecoder().decode(WorkflowRunCheckpoint.self, from: encoded)
        XCTAssertEqual(decoded, recovery[0])
    }
}
