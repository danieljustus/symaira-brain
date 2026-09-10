import Darwin
import Dispatch
import Foundation

/// The outcome of a subprocess that was given a finite execution budget.
public struct BoundedProcessResult: Sendable {
    public let standardOutput: Data
    public let standardError: Data
    public let terminationStatus: Int32
    public let timedOut: Bool
    /// True when the child produced more than the run's output budget and the
    /// captured bytes were cut short. `output` is then a prefix, so callers
    /// must not parse it as a complete document.
    public let truncated: Bool

    public init(
        standardOutput: Data,
        standardError: Data,
        terminationStatus: Int32,
        timedOut: Bool,
        truncated: Bool = false
    ) {
        self.standardOutput = standardOutput
        self.standardError = standardError
        self.terminationStatus = terminationStatus
        self.timedOut = timedOut
        self.truncated = truncated
    }

    public var output: String {
        String(data: standardOutput, encoding: .utf8) ?? ""
    }
}

public enum BoundedProcessRunnerError: Error, LocalizedError, Sendable, Equatable {
    case executableUnavailable(String)
    case standardInputWriteFailed

    public var errorDescription: String? {
        switch self {
        case .executableUnavailable(let executable):
            return "Executable not found or not executable: \(executable)"
        case .standardInputWriteFailed:
            return "Could not write subprocess standard input"
        }
    }
}

/// Runs local commands with bounded lifetime and captured output.
///
/// A bare executable name is resolved against the supplied environment's
/// `PATH` first, then against a small, fixed list of standard install
/// locations (Homebrew's prefixes and the managed `~/.symaira/bin`) — never
/// against a shell-resolved or otherwise user-configurable path. The fallback
/// exists because a GUI app launched by launchd inherits launchd's minimal
/// PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), which never includes Homebrew —
/// an interactive shell only adds it via `.zprofile`/`.zshrc`. Without it, sibling
/// Symaira CLIs (`symbrain`, `symvault`) installed the documented way are
/// invisible to the app even though they work fine from a terminal. Absolute
/// paths are accepted for callers that already own a system path. Standard
/// output and standard error are drained concurrently while the child is
/// running.
public enum BoundedProcessRunner {
    /// Default ceiling on the bytes captured from a child's standard output
    /// and standard error, each. The timeout bounds how long a child runs; it
    /// does not bound how much a wedged or misbehaving child can write into
    /// this process's memory within that budget.
    public static let defaultMaximumOutputBytes = 8 * 1024 * 1024

    /// Resolves `executable` to an absolute path using the same PATH-then-
    /// fallback search ``run(executable:arguments:timeoutSeconds:environment:standardInput:)``
    /// uses internally, without spawning anything. Callers that need to embed
    /// an absolute path in another process's command line (e.g. inside a
    /// privileged `do shell script`, whose own environment cannot be trusted
    /// to resolve it) resolve it here first.
    public static func resolveExecutablePath(
        _ executable: String,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> String? {
        resolveCandidates(executable, environment: environment).first
    }

    /// Every location `executable` resolves to, in the same PATH-then-fallback
    /// order, instead of only the first.
    ///
    /// Ordinary execution wants the first hit and nothing else. A caller that
    /// additionally *rejects* candidates — the privileged resolver, which
    /// requires root ownership — must be able to keep looking: `/opt/homebrew/bin`
    /// is searched before `/usr/local/bin`, so stopping at the first hit means a
    /// Homebrew install (user-owned on Apple Silicon, and therefore refused)
    /// permanently masks a correctly installed root-owned binary further down
    /// the list.
    public static func resolveExecutableCandidates(
        _ executable: String,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> [String] {
        resolveCandidates(executable, environment: environment)
    }

    public static func run(
        executable: String,
        arguments: [String] = [],
        timeoutSeconds: TimeInterval = 3,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        standardInput: Data? = nil,
        maximumOutputBytes: Int = defaultMaximumOutputBytes
    ) throws -> BoundedProcessResult {
        guard let executablePath = resolve(executable, environment: environment) else {
            throw BoundedProcessRunnerError.executableUnavailable(executable)
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: executablePath)
        process.arguments = arguments
        process.environment = environment

        let inputPipe = Pipe()
        let outputPipe = Pipe()
        let errorPipe = Pipe()
        process.standardInput = inputPipe
        process.standardOutput = outputPipe
        process.standardError = errorPipe

        let terminationSemaphore = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in
            terminationSemaphore.signal()
        }

        do {
            try process.run()
        } catch {
            throw BoundedProcessRunnerError.executableUnavailable(executable)
        }
        let processGroupID = process.processIdentifier
        let hasDedicatedProcessGroup = setpgid(processGroupID, processGroupID) == 0

        let outputCollector = DataCollector(limit: maximumOutputBytes)
        let errorCollector = DataCollector(limit: maximumOutputBytes)
        let readerGroup = DispatchGroup()
        readerGroup.enter()
        readerGroup.enter()

        // Register both readers immediately after launch. A child can fill
        // either pipe while it is still running, so post-exit reads can deadlock.
        let outputReader = PipeReader(
            handle: outputPipe.fileHandleForReading,
            collector: outputCollector,
            completion: readerGroup
        )
        let errorReader = PipeReader(
            handle: errorPipe.fileHandleForReading,
            collector: errorCollector,
            completion: readerGroup
        )

        do {
            if let standardInput {
                try inputPipe.fileHandleForWriting.write(contentsOf: standardInput)
            }
            try inputPipe.fileHandleForWriting.close()
        } catch {
            terminateAndReap(
                process,
                processGroupID: processGroupID,
                hasDedicatedProcessGroup: hasDedicatedProcessGroup,
                terminationSemaphore: terminationSemaphore
            )
            waitForReaders(
                readerGroup,
                outputReader: outputReader,
                errorReader: errorReader
            )
            throw BoundedProcessRunnerError.standardInputWriteFailed
        }

        let waitResult = terminationSemaphore.wait(timeout: .now() + max(0, timeoutSeconds))
        let timedOut = waitResult == .timedOut
        if timedOut {
            terminateAndReap(
                process,
                processGroupID: processGroupID,
                hasDedicatedProcessGroup: hasDedicatedProcessGroup,
                terminationSemaphore: terminationSemaphore
            )
        } else {
            // waitUntilExit is the explicit reap for naturally completed
            // children; the termination handler only provides bounded waiting.
            process.waitUntilExit()
        }
        process.terminationHandler = nil

        // On timeout the process group is gone before waiting for EOF, so all
        // inherited pipe writers have been closed and both readers can finish.
        waitForReaders(
            readerGroup,
            outputReader: outputReader,
            errorReader: errorReader,
            timeout: timedOut ? .milliseconds(100) : .milliseconds(500)
        )
        return BoundedProcessResult(
            standardOutput: outputCollector.value,
            standardError: errorCollector.value,
            terminationStatus: process.terminationStatus,
            timedOut: timedOut,
            truncated: outputCollector.truncated || errorCollector.truncated
        )
    }

    public static func runAsync(
        executable: String,
        arguments: [String] = [],
        timeoutSeconds: TimeInterval = 3,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        standardInput: Data? = nil,
        maximumOutputBytes: Int = defaultMaximumOutputBytes
    ) async throws -> BoundedProcessResult {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                do {
                    continuation.resume(returning: try run(
                        executable: executable,
                        arguments: arguments,
                        timeoutSeconds: timeoutSeconds,
                        environment: environment,
                        standardInput: standardInput,
                        maximumOutputBytes: maximumOutputBytes
                    ))
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private static func resolve(_ executable: String, environment: [String: String]) -> String? {
        resolveCandidates(executable, environment: environment).first
    }

    /// All matching executables, in search order, with duplicates removed —
    /// PATH commonly repeats a directory that the fallback list also names.
    private static func resolveCandidates(_ executable: String, environment: [String: String]) -> [String] {
        if executable.hasPrefix("/") {
            return FileManager.default.isExecutableFile(atPath: executable) ? [executable] : []
        }
        var directories: [String] = []
        if let path = environment["PATH"] {
            directories += path.split(separator: ":", omittingEmptySubsequences: true).map(String.init)
        }
        directories += fallbackDirectories(environment: environment)

        var candidates: [String] = []
        var seen: Set<String> = []
        for directory in directories {
            let candidate = "\(directory)/\(executable)"
            guard seen.insert(candidate).inserted else { continue }
            if FileManager.default.isExecutableFile(atPath: candidate) {
                candidates.append(candidate)
            }
        }
        return candidates
    }

    /// Fixed, non-configurable install roots checked only after a PATH-based
    /// lookup fails. This is deliberately a short allowlist of well-known
    /// locations — not the caller's full inherited environment — so the
    /// widened search stays auditable and cannot be redirected by anything
    /// the process happens to inherit.
    private static func fallbackDirectories(environment: [String: String]) -> [String] {
        var directories = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/local/sbin"]
        if let home = environment["HOME"] ?? ProcessInfo.processInfo.environment["HOME"] {
            directories.append("\(home)/.symaira/bin")
        }
        return directories
    }

    private static func terminateAndReap(
        _ process: Process,
        processGroupID: Int32,
        hasDedicatedProcessGroup: Bool,
        terminationSemaphore: DispatchSemaphore
    ) {
        process.terminate()
        if terminationSemaphore.wait(timeout: .now() + .milliseconds(250)) == .timedOut {
            _ = kill(process.processIdentifier, SIGKILL)
            if hasDedicatedProcessGroup {
                _ = kill(-processGroupID, SIGKILL)
            }
        }
        // The child is guaranteed to have been signalled before waiting. This
        // collects it instead of leaving a zombie behind after a timeout.
        process.waitUntilExit()
    }

    private static func waitForReaders(
        _ readerGroup: DispatchGroup,
        outputReader: PipeReader,
        errorReader: PipeReader,
        timeout: DispatchTimeInterval = .milliseconds(500)
    ) {
        guard readerGroup.wait(timeout: .now() + timeout) == .timedOut else {
            return
        }
        // A failed process-group setup can leave an inherited writer alive.
        // Stop handlers after the bounded grace period rather than hanging the
        // runner indefinitely; bytes already delivered remain captured.
        outputReader.cancel()
        errorReader.cancel()
    }
}

/// Accumulates a child's output up to a fixed byte budget. Past the budget the
/// surplus is dropped rather than buffered, so a child that writes without
/// bound within its time budget cannot grow this process's memory without
/// bound. Dropping is recorded so callers can refuse to parse a partial
/// document instead of silently misreading a cut-off one.
private final class DataCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    private var didTruncate = false
    private let limit: Int

    init(limit: Int) {
        self.limit = max(0, limit)
    }

    func append(_ data: Data) {
        lock.lock()
        defer { lock.unlock() }
        let remaining = limit - self.data.count
        guard remaining > 0 else {
            if !data.isEmpty { didTruncate = true }
            return
        }
        if data.count <= remaining {
            self.data.append(data)
        } else {
            self.data.append(data.prefix(remaining))
            didTruncate = true
        }
    }

    var value: Data {
        lock.lock()
        defer { lock.unlock() }
        return data
    }

    var truncated: Bool {
        lock.lock()
        defer { lock.unlock() }
        return didTruncate
    }
}

private final class PipeReader: @unchecked Sendable {
    private let handle: FileHandle
    private let collector: DataCollector
    private let completion: DispatchGroup
    private let stateLock = NSLock()
    private var finished = false

    init(handle: FileHandle, collector: DataCollector, completion: DispatchGroup) {
        self.handle = handle
        self.collector = collector
        self.completion = completion
        handle.readabilityHandler = { [weak self] handle in
            self?.consume(handle)
        }
    }

    private func consume(_ handle: FileHandle) {
        stateLock.lock()
        let isFinished = finished
        stateLock.unlock()
        guard !isFinished else { return }

        let data = handle.availableData
        if !data.isEmpty {
            collector.append(data)
            return
        }

        finish(handle: handle)
    }

    private func finish(handle: FileHandle) {
        stateLock.lock()
        guard !finished else {
            stateLock.unlock()
            return
        }
        finished = true
        stateLock.unlock()

        handle.readabilityHandler = nil
        completion.leave()
    }

    func cancel() {
        stateLock.lock()
        guard !finished else {
            stateLock.unlock()
            return
        }
        finished = true
        stateLock.unlock()

        handle.readabilityHandler = nil
        completion.leave()
    }
}
