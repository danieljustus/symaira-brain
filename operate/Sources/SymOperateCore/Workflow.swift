import Foundation

/// The terminal state of a workflow run.
public enum WorkflowRunStatus: String, Codable, Sendable {
    case completed
    case failed
    case cancelled
    case invalid
}

/// The terminal state recorded for each ordered workflow step.
public enum WorkflowStepStatus: String, Codable, Sendable {
    case completed
    case failed
    case skipped
    case cancelled
}

/// One action in a verified workflow. Conditions are kept on the step so the
/// executor can pass them directly to the existing AutomationController APIs.
public struct WorkflowStep: Codable, Sendable, Equatable, Identifiable {
    public let id: String
    public let action: String
    public let parameters: [String: String]
    public let conditions: ActionConditions
    /// Explicit recovery metadata. No recovery action is inferred by the runner.
    public let recovery: WorkflowRecovery

    public init(
        id: String,
        action: String,
        parameters: [String: String] = [:],
        conditions: ActionConditions = ActionConditions(),
        recovery: WorkflowRecovery = .unavailable
    ) {
        self.id = id
        self.action = action
        self.parameters = parameters
        self.conditions = conditions
        self.recovery = recovery
    }

    public init(
        id: String,
        action: String,
        parameters: [String: String] = [:],
        precondition: UIElementPredicate? = nil,
        postcondition: UIElementPredicate? = nil,
        recovery: WorkflowRecovery = .unavailable
    ) {
        self.init(
            id: id,
            action: action,
            parameters: parameters,
            conditions: ActionConditions(precondition: precondition, postcondition: postcondition),
            recovery: recovery
        )
    }

    private enum CodingKeys: String, CodingKey {
        case id, action, parameters, conditions, recovery
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        action = try container.decode(String.self, forKey: .action)
        parameters = try container.decodeIfPresent([String: String].self, forKey: .parameters) ?? [:]
        conditions = try container.decodeIfPresent(ActionConditions.self, forKey: .conditions) ?? ActionConditions()
        recovery = try container.decodeIfPresent(WorkflowRecovery.self, forKey: .recovery) ?? .unavailable
    }
}

/// An ordered, deliberately bounded workflow plan.
public struct WorkflowPlan: Codable, Sendable, Equatable {
    public static let maxSteps = 32
    public static let maxCompensations = 32

    public let id: String
    public let steps: [WorkflowStep]

    public init(id: String = "workflow", steps: [WorkflowStep]) {
        self.id = id
        self.steps = steps
    }

    public var compensationCount: Int {
        steps.reduce(into: 0) { count, step in
            if step.recovery.isAvailable { count += 1 }
        }
    }

    public var isWithinBound: Bool {
        steps.count <= Self.maxSteps && compensationCount <= Self.maxCompensations
    }
}

/// Identifies whether an event belongs to the forward workflow or recovery path.
public enum WorkflowHistoryPhase: String, Codable, Sendable {
    case execution
    case compensation
}

/// A redacted event emitted after each attempted or skipped step.
public struct WorkflowHistoryEvent: Codable, Sendable, Equatable {
    public let planID: String
    public let stepID: String
    public let action: String
    public let status: WorkflowStepStatus
    public let parameters: [String: String]
    public let message: String
    public let phase: WorkflowHistoryPhase
    /// The forward step being recovered when `phase` is `.compensation`.
    public let originalStepID: String?

    public init(
        planID: String,
        stepID: String,
        action: String,
        status: WorkflowStepStatus,
        parameters: [String: String] = [:],
        message: String,
        phase: WorkflowHistoryPhase = .execution,
        originalStepID: String? = nil
    ) {
        let redactedParameters = Self.redact(parameters)
        self.planID = planID
        self.stepID = stepID
        self.action = action
        self.status = status
        self.parameters = redactedParameters
        self.message = Self.redact(message, sensitiveValues: parameters.compactMap { key, value in
            Self.isSensitiveKey(key) ? value : nil
        })
        self.phase = phase
        self.originalStepID = originalStepID
    }

    private enum CodingKeys: String, CodingKey {
        case planID, stepID, action, status, parameters, message, phase, originalStepID
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        planID = try container.decode(String.self, forKey: .planID)
        stepID = try container.decode(String.self, forKey: .stepID)
        action = try container.decode(String.self, forKey: .action)
        status = try container.decode(WorkflowStepStatus.self, forKey: .status)
        parameters = try container.decodeIfPresent([String: String].self, forKey: .parameters) ?? [:]
        message = try container.decode(String.self, forKey: .message)
        phase = try container.decodeIfPresent(WorkflowHistoryPhase.self, forKey: .phase) ?? .execution
        originalStepID = try container.decodeIfPresent(String.self, forKey: .originalStepID)
    }

    private static let sensitiveKeyFragments = [
        "password", "secret", "token", "credential", "authorization", "api_key", "apikey", "text"
    ]

    private static func isSensitiveKey(_ key: String) -> Bool {
        let normalized = key.lowercased().replacingOccurrences(of: "-", with: "_")
        return sensitiveKeyFragments.contains { normalized.contains($0) }
    }

    private static func redact(_ parameters: [String: String]) -> [String: String] {
        parameters.reduce(into: [:]) { result, entry in
            result[entry.key] = isSensitiveKey(entry.key) ? "<redacted>" : entry.value
        }
    }

    private static func redact(_ message: String, sensitiveValues: [String]) -> String {
        sensitiveValues.reduce(message) { value, secret in
            guard !secret.isEmpty else { return value }
            return value.replacingOccurrences(of: secret, with: "<redacted>")
        }
    }
}

public typealias WorkflowHistoryHook = @Sendable (WorkflowHistoryEvent) -> Void

/// The result of one step, retained in plan order for deterministic aggregation.
public struct WorkflowStepResult: Codable, Sendable {
    public let stepID: String
    public let action: String
    public let status: WorkflowStepStatus
    public let result: ActionResult?
    public let message: String?

    public init(
        stepID: String,
        action: String,
        status: WorkflowStepStatus,
        result: ActionResult? = nil,
        message: String? = nil
    ) {
        self.stepID = stepID
        self.action = action
        self.status = status
        self.result = result
        self.message = message
    }
}

/// Deterministically aggregates the completed prefix, skipped suffix, and
/// any bounded compensation attempts.
public struct WorkflowResult: Codable, Sendable {
    public let planID: String
    public let status: WorkflowRunStatus
    public let steps: [WorkflowStepResult]
    public let failureStepID: String?
    public let message: String?
    public let compensations: [WorkflowCompensationResult]

    public init(
        planID: String,
        status: WorkflowRunStatus,
        steps: [WorkflowStepResult],
        failureStepID: String? = nil,
        message: String? = nil,
        compensations: [WorkflowCompensationResult] = []
    ) {
        self.planID = planID
        self.status = status
        self.steps = steps
        self.failureStepID = failureStepID
        self.message = message
        self.compensations = compensations
    }

    private enum CodingKeys: String, CodingKey {
        case planID, status, steps, failureStepID, message, compensations
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        planID = try container.decode(String.self, forKey: .planID)
        status = try container.decode(WorkflowRunStatus.self, forKey: .status)
        steps = try container.decode([WorkflowStepResult].self, forKey: .steps)
        failureStepID = try container.decodeIfPresent(String.self, forKey: .failureStepID)
        message = try container.decodeIfPresent(String.self, forKey: .message)
        compensations = try container.decodeIfPresent([WorkflowCompensationResult].self, forKey: .compensations) ?? []
    }

    public var completedStepCount: Int {
        steps.lazy.filter { $0.status == .completed }.count
    }
}

/// Runs a plan serially. A step must return a confirmed ActionResult before
/// the next step starts; failures, unverifiable results, and cancellation abort
/// the remaining suffix and run explicit compensations for completed steps.
public struct WorkflowRunner: Sendable {
    public typealias Executor = @Sendable (WorkflowStep) async throws -> ActionResult
    public typealias CompensationExecutor = @Sendable (WorkflowCompensation) async throws -> ActionResult
    public typealias RecoveryExecutor = CompensationExecutor
    public typealias RecoveryPolicy = @Sendable (WorkflowCompensation) -> Bool

    private let executor: Executor
    private let compensationExecutor: CompensationExecutor
    private let recoveryPolicy: RecoveryPolicy?
    private let history: WorkflowHistoryHook?

    public init(
        executor: @escaping Executor,
        compensationExecutor: CompensationExecutor? = nil,
        recoveryPolicy: RecoveryPolicy? = nil,
        history: WorkflowHistoryHook? = nil
    ) {
        self.executor = executor
        self.compensationExecutor = compensationExecutor ?? { compensation in
            try await executor(WorkflowStep(
                id: "compensation",
                action: compensation.action,
                parameters: compensation.parameters,
                conditions: compensation.conditions
            ))
        }
        self.recoveryPolicy = recoveryPolicy
        self.history = history
    }

    /// Compatibility spelling for callers that call the hook a compensator.
    public init(
        executor: @escaping Executor,
        compensator: @escaping CompensationExecutor,
        recoveryPolicy: RecoveryPolicy? = nil,
        history: WorkflowHistoryHook? = nil
    ) {
        self.init(
            executor: executor,
            compensationExecutor: compensator,
            recoveryPolicy: recoveryPolicy,
            history: history
        )
    }

    public func run(_ plan: WorkflowPlan) async -> WorkflowResult {
        guard plan.isWithinBound else {
            let message = "Workflow exceeds the maximum of \(WorkflowPlan.maxSteps) steps or \(WorkflowPlan.maxCompensations) compensations."
            let skipped = plan.steps.map { step in
                let result = WorkflowStepResult(
                    stepID: step.id,
                    action: step.action,
                    status: .skipped,
                    message: message
                )
                emit(planID: plan.id, step: step, status: .skipped, message: message)
                return result
            }
            return WorkflowResult(planID: plan.id, status: .invalid, steps: skipped, message: message)
        }

        guard plan.steps.allSatisfy({ $0.conditions.precondition != nil || $0.conditions.postcondition != nil }) else {
            let message = "Every workflow step must declare a precondition or postcondition."
            let skipped = plan.steps.map { step in
                let result = WorkflowStepResult(
                    stepID: step.id,
                    action: step.action,
                    status: .skipped,
                    message: message
                )
                emit(planID: plan.id, step: step, status: .skipped, message: message)
                return result
            }
            return WorkflowResult(planID: plan.id, status: .invalid, steps: skipped, message: message)
        }

        var stepResults: [WorkflowStepResult] = []
        stepResults.reserveCapacity(plan.steps.count)

        for (index, step) in plan.steps.enumerated() {
            if Task.isCancelled {
                return await abort(
                    plan: plan,
                    results: &stepResults,
                    nextIndex: index,
                    status: .cancelled,
                    message: "Workflow was cancelled."
                )
            }

            do {
                let actionResult = try await executor(step)
                guard actionResult.ok,
                      actionResult.effect == .confirmed,
                      actionResult.verification.status == .confirmed else {
                    let message = "Step did not produce a confirmed, verifiable result."
                    let stepResult = WorkflowStepResult(
                        stepID: step.id,
                        action: step.action,
                        status: .failed,
                        result: actionResult,
                        message: message
                    )
                    stepResults.append(stepResult)
                    emit(planID: plan.id, step: step, status: .failed, message: message)
                    return await abort(
                        plan: plan,
                        results: &stepResults,
                        nextIndex: index + 1,
                        status: .failed,
                        failureStepID: step.id,
                        message: message
                    )
                }

                let stepResult = WorkflowStepResult(
                    stepID: step.id,
                    action: step.action,
                    status: .completed,
                    result: actionResult
                )
                stepResults.append(stepResult)
                emit(planID: plan.id, step: step, status: .completed, message: "Step completed.")
            } catch is CancellationError {
                let message = "Workflow was cancelled."
                stepResults.append(WorkflowStepResult(stepID: step.id, action: step.action, status: .cancelled, message: message))
                emit(planID: plan.id, step: step, status: .cancelled, message: message)
                return await abort(
                    plan: plan,
                    results: &stepResults,
                    nextIndex: index + 1,
                    status: .cancelled,
                    failureStepID: step.id,
                    message: message
                )
            } catch {
                let message = Self.redacted(error.localizedDescription, values: step.parameters.values)
                stepResults.append(WorkflowStepResult(stepID: step.id, action: step.action, status: .failed, message: message))
                emit(planID: plan.id, step: step, status: .failed, message: message)
                return await abort(
                    plan: plan,
                    results: &stepResults,
                    nextIndex: index + 1,
                    status: .failed,
                    failureStepID: step.id,
                    message: message
                )
            }
        }

        return WorkflowResult(planID: plan.id, status: .completed, steps: stepResults)
    }

    private func abort(
        plan: WorkflowPlan,
        results: inout [WorkflowStepResult],
        nextIndex: Int,
        status: WorkflowRunStatus,
        failureStepID: String? = nil,
        message: String
    ) async -> WorkflowResult {
        for step in plan.steps.dropFirst(nextIndex) {
            results.append(WorkflowStepResult(stepID: step.id, action: step.action, status: .skipped, message: "Skipped after workflow abort."))
            emit(planID: plan.id, step: step, status: .skipped, message: "Skipped after workflow abort.")
        }
        let compensations = await compensate(plan: plan, results: results)
        return WorkflowResult(
            planID: plan.id,
            status: status,
            steps: results,
            failureStepID: failureStepID,
            message: message,
            compensations: compensations
        )
    }

    private func compensate(plan: WorkflowPlan, results: [WorkflowStepResult]) async -> [WorkflowCompensationResult] {
        let completed = results.reversed().compactMap { stepResult -> WorkflowStep? in
            guard stepResult.status == .completed else { return nil }
            return plan.steps.first(where: { $0.id == stepResult.stepID })
        }

        var outcomes: [WorkflowCompensationResult] = []
        outcomes.reserveCapacity(completed.count)
        for step in completed {
            guard let compensation = step.recovery.compensation else {
                let message = "No verified recovery action is available for this step."
                outcomes.append(WorkflowCompensationResult(
                    stepID: step.id,
                    action: step.action,
                    status: .unavailable,
                    message: message
                ))
                history?(WorkflowHistoryEvent(
                    planID: plan.id,
                    stepID: step.id,
                    action: "recovery_unavailable",
                    status: .skipped,
                    message: message,
                    phase: .compensation,
                    originalStepID: step.id
                ))
                continue
            }

            if let recoveryPolicy, !recoveryPolicy(compensation) {
                let message = "Recovery action was refused by policy."
                outcomes.append(WorkflowCompensationResult(
                    stepID: step.id,
                    action: compensation.action,
                    status: .refused,
                    message: message
                ))
                emitCompensation(planID: plan.id, step: step, compensation: compensation, status: .failed, message: message)
                continue
            }

            do {
                let actionResult = try await compensationExecutor(compensation)
                guard actionResult.ok,
                      actionResult.effect == .confirmed,
                      actionResult.verification.status == .confirmed else {
                    let message = "Recovery action did not produce a confirmed, verifiable result."
                    outcomes.append(WorkflowCompensationResult(
                        stepID: step.id,
                        action: compensation.action,
                        status: actionResult.effect == .refused ? .refused : .failed,
                        result: actionResult,
                        message: message
                    ))
                    emitCompensation(planID: plan.id, step: step, compensation: compensation, status: .failed, message: message)
                    continue
                }
                let message = "Recovery action completed."
                outcomes.append(WorkflowCompensationResult(
                    stepID: step.id,
                    action: compensation.action,
                    status: .completed,
                    result: actionResult,
                    message: message
                ))
                emitCompensation(planID: plan.id, step: step, compensation: compensation, status: .completed, message: message)
            } catch {
                let message = Self.redacted(error.localizedDescription, values: compensation.parameters.values)
                outcomes.append(WorkflowCompensationResult(
                    stepID: step.id,
                    action: compensation.action,
                    status: .failed,
                    message: message
                ))
                emitCompensation(planID: plan.id, step: step, compensation: compensation, status: .failed, message: message)
            }
        }
        return outcomes
    }

    private func emit(planID: String, step: WorkflowStep, status: WorkflowStepStatus, message: String) {
        history?(WorkflowHistoryEvent(
            planID: planID,
            stepID: step.id,
            action: step.action,
            status: status,
            parameters: step.parameters,
            message: message,
            phase: .execution
        ))
    }

    private func emitCompensation(
        planID: String,
        step: WorkflowStep,
        compensation: WorkflowCompensation,
        status: WorkflowStepStatus,
        message: String
    ) {
        history?(WorkflowHistoryEvent(
            planID: planID,
            stepID: step.id,
            action: compensation.action,
            status: status,
            parameters: compensation.parameters,
            message: message,
            phase: .compensation,
            originalStepID: step.id
        ))
    }

    private static func redacted(_ message: String, values: Dictionary<String, String>.Values) -> String {
        values.reduce(message) { message, value in
            value.isEmpty ? message : message.replacingOccurrences(of: value, with: "<redacted>")
        }
    }
}
