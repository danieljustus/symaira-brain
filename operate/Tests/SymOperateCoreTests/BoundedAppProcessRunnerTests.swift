import XCTest
@testable import SymOperateCore

final class BoundedAppProcessRunnerTests: XCTestCase {
    func testAdapterPreservesSuccessfulCompletion() throws {
        let result = try BoundedAppProcessRunner.run(
            executable: "/usr/bin/printf",
            arguments: ["ok"],
            timeoutSeconds: 1
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
    }

    func testAdapterPreservesTimeoutResult() throws {
        let result = try BoundedAppProcessRunner.run(
            executable: "/bin/sleep",
            arguments: ["2"],
            timeoutSeconds: 0.05
        )

        XCTAssertTrue(result.timedOut)
        XCTAssertNotEqual(result.terminationStatus, 0)
    }
}
