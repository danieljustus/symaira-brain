import Foundation
import XCTest
@testable import SymCockpitHistory

final class BoundedProcessRunnerAsyncTests: XCTestCase {
    func testAsyncAPIUsesPATHAndPreservesCapturedOutput() async throws {
        let result = try await BoundedProcessRunner.runAsync(
            executable: "printf",
            arguments: ["async output"],
            timeoutSeconds: 1,
            environment: ["PATH": "/usr/bin:/bin"]
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(result.output, "async output")
    }
}
