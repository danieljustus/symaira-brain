import Foundation
import SymCockpitHistory

struct BoundedAppProcessResult: Sendable {
    let terminationStatus: Int32
    let timedOut: Bool
}

/// Operate's legacy result shape backed by the shared process runner.
enum BoundedAppProcessRunner {
    static func run(executable: String, arguments: [String], timeoutSeconds: TimeInterval) throws -> BoundedAppProcessResult {
        let result = try SymCockpitHistory.BoundedProcessRunner.run(
            executable: executable,
            arguments: arguments,
            timeoutSeconds: timeoutSeconds
        )
        return BoundedAppProcessResult(
            terminationStatus: result.terminationStatus,
            timedOut: result.timedOut
        )
    }
}
