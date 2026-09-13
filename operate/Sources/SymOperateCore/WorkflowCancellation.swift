import Foundation

/// Lifecycle state persisted for a bounded workflow execution.
public enum WorkflowRunState: String, Codable, Sendable {
    case running
    case completed
    case failed
    case cancelled
    case timedOut = "timed_out"
    case interrupted
    case invalid

    public var isIncomplete: Bool { self == .running }
}

/// A cooperative, bounded deadline shared by every step in a workflow.
public struct WorkflowDeadline: Codable, Equatable, Sendable {
    public static let defaultTimeoutSeconds: TimeInterval = 60
    public static let maximumTimeoutSeconds: TimeInterval = 15 * 60

    public let startedAt: Date
    public let deadline: Date

    public init(timeoutSeconds: TimeInterval = Self.defaultTimeoutSeconds, now: Date = Date()) {
        let bounded = min(max(0, timeoutSeconds), Self.maximumTimeoutSeconds)
        self.startedAt = now
        self.deadline = now.addingTimeInterval(bounded)
    }

    public init(startedAt: Date, deadline: Date) {
        self.startedAt = startedAt
        self.deadline = deadline
    }

    public var remainingTime: TimeInterval { max(0, deadline.timeIntervalSinceNow) }
    public var isExpired: Bool { Date() >= deadline }

    public func check() throws {
        if Task.isCancelled { throw WorkflowRunError.cancelled }
        if isExpired { throw WorkflowRunError.deadlineExceeded }
    }

    public func sleep(for duration: TimeInterval) async throws {
        try check()
        let bounded = min(max(0, duration), remainingTime)
        try await Task.sleep(nanoseconds: UInt64(bounded * 1_000_000_000))
        try check()
    }
}

public enum WorkflowRunError: LocalizedError, Equatable, Sendable {
    case cancelled
    case deadlineExceeded

    public var errorDescription: String? {
        switch self {
        case .cancelled:
            return "Workflow was cancelled before the next side effect."
        case .deadlineExceeded:
            return "Workflow deadline expired; no subsequent side effect was attempted."
        }
    }
}

/// Minimal state written before and after each step. A running checkpoint is
/// deliberately not resumable: restart recovery marks it interrupted instead
/// of replaying an action whose effect may be ambiguous.
public struct WorkflowRunCheckpoint: Codable, Equatable, Sendable {
    public let runID: String
    public let planID: String
    public let nextStepIndex: Int
    public let completedStepIDs: [String]
    public let currentStepID: String?
    public let state: WorkflowRunState
    public let updatedAt: Date
    public let message: String?

    public init(
        runID: String,
        planID: String,
        nextStepIndex: Int,
        completedStepIDs: [String],
        currentStepID: String? = nil,
        state: WorkflowRunState,
        updatedAt: Date = Date(),
        message: String? = nil
    ) {
        self.runID = runID
        self.planID = planID
        self.nextStepIndex = max(0, nextStepIndex)
        self.completedStepIDs = completedStepIDs
        self.currentStepID = currentStepID
        self.state = state
        self.updatedAt = updatedAt
        self.message = message
    }

    public var isIncomplete: Bool { state.isIncomplete }

    public func markingInterrupted(message: String = "The process ended before this workflow completed; no action was resumed automatically.") -> WorkflowRunCheckpoint {
        WorkflowRunCheckpoint(
            runID: runID,
            planID: planID,
            nextStepIndex: nextStepIndex,
            completedStepIDs: completedStepIDs,
            currentStepID: currentStepID,
            state: .interrupted,
            message: message
        )
    }
}

public protocol WorkflowRunCheckpointStore: Sendable {
    func save(_ checkpoint: WorkflowRunCheckpoint) throws
    func load(runID: String) throws -> WorkflowRunCheckpoint?
    func all() throws -> [WorkflowRunCheckpoint]
}

/// Thread-safe test store and a useful host-app persistence seam.
public final class InMemoryWorkflowRunCheckpointStore: WorkflowRunCheckpointStore, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: WorkflowRunCheckpoint] = [:]

    public init() {}

    public func save(_ checkpoint: WorkflowRunCheckpoint) throws {
        lock.lock()
        values[checkpoint.runID] = checkpoint
        lock.unlock()
    }

    public func load(runID: String) throws -> WorkflowRunCheckpoint? {
        lock.lock()
        defer { lock.unlock() }
        return values[runID]
    }

    public func all() throws -> [WorkflowRunCheckpoint] {
        lock.lock()
        defer { lock.unlock() }
        return values.values.sorted { $0.updatedAt < $1.updatedAt }
    }
}

/// Atomic JSON persistence for bounded run state. The single file is capped so
/// abandoned runs cannot grow without limit.
public final class FileWorkflowRunCheckpointStore: WorkflowRunCheckpointStore, @unchecked Sendable {
    public let fileURL: URL
    public let maxRuns: Int
    private let lock = NSLock()

    public init(fileURL: URL, maxRuns: Int = 128) {
        self.fileURL = fileURL
        self.maxRuns = max(1, maxRuns)
        try? FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fileURL.deletingLastPathComponent().path)
    }

    public func save(_ checkpoint: WorkflowRunCheckpoint) throws {
        lock.lock()
        defer { lock.unlock() }
        var values = try readLocked()
        values[checkpoint.runID] = checkpoint
        if values.count > maxRuns {
            let excess = values.values.sorted { $0.updatedAt < $1.updatedAt }.prefix(values.count - maxRuns)
            for old in excess { values.removeValue(forKey: old.runID) }
        }
        let data = try Self.encoder.encode(values)
        try data.write(to: fileURL, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: fileURL.path)
    }

    public func load(runID: String) throws -> WorkflowRunCheckpoint? {
        lock.lock()
        defer { lock.unlock() }
        return try readLocked()[runID]
    }

    public func all() throws -> [WorkflowRunCheckpoint] {
        lock.lock()
        defer { lock.unlock() }
        return try readLocked().values.sorted { $0.updatedAt < $1.updatedAt }
    }

    private func readLocked() throws -> [String: WorkflowRunCheckpoint] {
        guard let data = try? Data(contentsOf: fileURL) else { return [:] }
        return try Self.decoder.decode([String: WorkflowRunCheckpoint].self, from: data)
    }

    private static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }()
    private static let decoder = JSONDecoder()
}

/// Safe restart handling: convert stale running records to explicit
/// interrupted records without executing or replaying any action.
public enum WorkflowRunRecovery {
    @discardableResult
    public static func recoverIncompleteRuns(from store: any WorkflowRunCheckpointStore) throws -> [WorkflowRunCheckpoint] {
        let incomplete = try store.all().filter(\.isIncomplete)
        return try incomplete.map { checkpoint in
            let recovered = checkpoint.markingInterrupted()
            try store.save(recovered)
            return recovered
        }
    }
}

public struct WorkflowExecutionResult: Sendable {
    public let runID: String
    public let state: WorkflowRunState
    public let workflow: WorkflowResult
    public let checkpoint: WorkflowRunCheckpoint?

    public var status: WorkflowRunState { state }
    public var completedStepCount: Int { workflow.completedStepCount }
    public var failClosed: Bool { state == .cancelled || state == .timedOut || state == .interrupted }

    public init(runID: String, state: WorkflowRunState, workflow: WorkflowResult, checkpoint: WorkflowRunCheckpoint?) {
        self.runID = runID
        self.state = state
        self.workflow = workflow
        self.checkpoint = checkpoint
    }
}

/// Deadline-aware wrapper around the verified #81 runner. It checkpoints the
/// run before a side effect, propagates cancellation to the step executor, and
/// never starts a later step after timeout or cancellation.
public struct DeadlineAwareWorkflowRunner: Sendable {
    public typealias Executor = @Sendable (WorkflowStep, WorkflowDeadline) async throws -> ActionResult

    private let executor: Executor

    public init(executor: @escaping WorkflowRunner.Executor) {
        self.executor = { step, _ in try await executor(step) }
    }

    public init(executor: @escaping Executor) {
        self.executor = executor
    }

    public func run(
        _ plan: WorkflowPlan,
        timeoutSeconds: TimeInterval = WorkflowDeadline.defaultTimeoutSeconds,
        runID: String = UUID().uuidString,
        checkpointStore: (any WorkflowRunCheckpointStore)? = nil
    ) async -> WorkflowExecutionResult {
        let deadline = WorkflowDeadline(timeoutSeconds: timeoutSeconds)
        let tracker = WorkflowRunTracker(runID: runID, planID: plan.id)

        if let checkpointStore {
            do {
                if let previous = try checkpointStore.load(runID: runID), previous.state == .running || previous.state == .interrupted {
                    let recovered = previous.state == .running ? previous.markingInterrupted() : previous
                    if previous.state == .running { try checkpointStore.save(recovered) }
                    let workflow = WorkflowResult(
                        planID: plan.id,
                        status: .failed,
                        steps: [],
                        failureStepID: recovered.currentStepID,
                        message: recovered.message
                    )
                    return WorkflowExecutionResult(runID: runID, state: .interrupted, workflow: workflow, checkpoint: recovered)
                }
            } catch {
                let message = "Workflow checkpoint storage failed; no action was attempted."
                let workflow = WorkflowResult(planID: plan.id, status: .failed, steps: [], message: message)
                return WorkflowExecutionResult(runID: runID, state: .failed, workflow: workflow, checkpoint: nil)
            }
        }

        do {
            try deadline.check()
            try save(await tracker.checkpoint(state: .running, nextStepIndex: 0, currentStepID: nil), to: checkpointStore)
        } catch WorkflowRunError.cancelled {
            return await finish(plan: plan, tracker: tracker, state: .cancelled, message: "Workflow was cancelled before execution.", workflow: emptyWorkflow(plan: plan), checkpointStore: checkpointStore)
        } catch WorkflowRunError.deadlineExceeded {
            return await finish(plan: plan, tracker: tracker, state: .timedOut, message: "Workflow deadline expired before execution.", workflow: emptyWorkflow(plan: plan), checkpointStore: checkpointStore)
        } catch {
            return await finish(plan: plan, tracker: tracker, state: .failed, message: "Workflow checkpoint storage failed; no action was attempted.", workflow: emptyWorkflow(plan: plan), checkpointStore: checkpointStore)
        }

        let stepExecutor = executor
        let runner = WorkflowRunner(executor: { [stepExecutor] step in
            do {
                try deadline.check()
                let index = await tracker.begin(stepID: step.id)
                try self.save(await tracker.checkpoint(state: .running, nextStepIndex: index, currentStepID: step.id), to: checkpointStore)
                let result = try await Self.execute(executor: stepExecutor, step: step, deadline: deadline)
                try deadline.check()
                await tracker.completed(stepID: step.id)
                try self.save(await tracker.checkpoint(state: .running, nextStepIndex: index + 1, currentStepID: nil), to: checkpointStore)
                return result
            } catch WorkflowRunError.cancelled {
                await tracker.setState(.cancelled)
                throw CancellationError()
            } catch WorkflowRunError.deadlineExceeded {
                await tracker.setState(.timedOut)
                throw WorkflowRunError.deadlineExceeded
            } catch is CancellationError {
                if deadline.isExpired {
                    await tracker.setState(.timedOut)
                    throw WorkflowRunError.deadlineExceeded
                }
                await tracker.setState(.cancelled)
                throw CancellationError()
            }
        })

        let workflow = await runner.run(plan)
        let trackedState = await tracker.state
        let state: WorkflowRunState
        if trackedState != .running {
            state = trackedState
        } else {
            state = switch workflow.status {
            case .completed: .completed
            case .cancelled: .cancelled
            case .invalid: .invalid
            case .failed: .failed
            }
        }
        let message = state == .timedOut
            ? "Workflow deadline expired; no subsequent step was attempted."
            : state == .cancelled
                ? "Workflow was cancelled; no subsequent step was attempted."
                : workflow.message
        return await finish(plan: plan, tracker: tracker, state: state, message: message, workflow: workflow, checkpointStore: checkpointStore)
    }

    public func recoverIncompleteRuns(from store: any WorkflowRunCheckpointStore) throws -> [WorkflowRunCheckpoint] {
        try WorkflowRunRecovery.recoverIncompleteRuns(from: store)
    }

    private static func execute(executor: @escaping Executor, step: WorkflowStep, deadline: WorkflowDeadline) async throws -> ActionResult {
        let actionTask = Task { try await executor(step, deadline) }
        do {
            return try await withTaskCancellationHandler(operation: {
                try await withThrowingTaskGroup(of: ActionResult.self) { group in
                    group.addTask { try await actionTask.value }
                    group.addTask {
                        try await deadline.sleep(for: deadline.remainingTime)
                        throw WorkflowRunError.deadlineExceeded
                    }
                    defer { group.cancelAll() }
                    return try await group.next()!
                }
            }, onCancel: {
                actionTask.cancel()
            })
        } catch {
            actionTask.cancel()
            throw error
        }
    }

    private func finish(
        plan: WorkflowPlan,
        tracker: WorkflowRunTracker,
        state: WorkflowRunState,
        message: String?,
        workflow: WorkflowResult,
        checkpointStore: (any WorkflowRunCheckpointStore)?
    ) async -> WorkflowExecutionResult {
        await tracker.setState(state)
        let checkpoint = await tracker.checkpoint(state: state, nextStepIndex: await tracker.nextStepIndex, currentStepID: await tracker.currentStepID, message: message)
        try? save(checkpoint, to: checkpointStore)
        return WorkflowExecutionResult(runID: checkpoint.runID, state: state, workflow: workflow, checkpoint: checkpointStore == nil ? nil : checkpoint)
    }

    private func save(_ checkpoint: WorkflowRunCheckpoint, to store: (any WorkflowRunCheckpointStore)?) throws {
        try store?.save(checkpoint)
    }

    private func emptyWorkflow(plan: WorkflowPlan) -> WorkflowResult {
        WorkflowResult(planID: plan.id, status: .cancelled, steps: [])
    }
}

private actor WorkflowRunTracker {
    let runID: String
    let planID: String
    private(set) var nextStepIndex = 0
    private(set) var currentStepID: String?
    private(set) var completedStepIDs: [String] = []
    private(set) var state: WorkflowRunState = .running

    init(runID: String, planID: String) {
        self.runID = runID
        self.planID = planID
    }

    func begin(stepID: String) -> Int {
        currentStepID = stepID
        return nextStepIndex
    }

    func completed(stepID: String) {
        if !completedStepIDs.contains(stepID) { completedStepIDs.append(stepID) }
        nextStepIndex += 1
        currentStepID = nil
    }

    func setState(_ state: WorkflowRunState) {
        self.state = state
    }

    func checkpoint(state: WorkflowRunState, nextStepIndex: Int, currentStepID: String?, message: String? = nil) -> WorkflowRunCheckpoint {
        WorkflowRunCheckpoint(runID: runID, planID: planID, nextStepIndex: nextStepIndex, completedStepIDs: completedStepIDs, currentStepID: currentStepID, state: state, message: message)
    }
}
