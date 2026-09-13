import Foundation
import XCTest
@testable import SymCockpitHistory

/// The runner bounded how long a child may run, but not how many bytes it may
/// buffer: `DataCollector` appended every chunk until EOF. A wedged or
/// misbehaving sibling CLI could therefore grow a long-lived menu-bar app's
/// memory by whatever it managed to write inside its time budget.
final class BoundedProcessRunnerOutputCapTests: XCTestCase {
    private static let path = ["PATH": "/bin:/usr/bin"]

    /// Emits `mebibytes` MiB on stdout as fast as `dd` can produce it.
    private func writer(mebibytes: Int) -> [String] {
        ["-c", "dd if=/dev/zero bs=1048576 count=\(mebibytes) 2>/dev/null | tr '\\000' O"]
    }

    func testOutputStopsAtTheCapAndReportsTruncation() throws {
        let cap = 64 * 1024
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: writer(mebibytes: 8),
            timeoutSeconds: 10,
            environment: Self.path,
            maximumOutputBytes: cap
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.standardOutput.count, cap, "collection must stop exactly at the cap")
        XCTAssertTrue(result.truncated, "truncation must be reported")
        XCTAssertTrue(result.standardOutput.allSatisfy { $0 == 0x4F }, "kept bytes stay intact")
    }

    /// The cap bounds memory: raising the child's total output by two orders of
    /// magnitude must not raise what the runner retains.
    func testRetainedBytesDoNotGrowWithTotalChildOutput() throws {
        let cap = 32 * 1024
        var counts: [Int] = []
        for mebibytes in [1, 16] {
            let result = try BoundedProcessRunner.run(
                executable: "/bin/sh",
                arguments: writer(mebibytes: mebibytes),
                timeoutSeconds: 15,
                environment: Self.path,
                maximumOutputBytes: cap
            )
            XCTAssertTrue(result.truncated)
            counts.append(result.standardOutput.count)
        }
        XCTAssertEqual(counts, [cap, cap], "retained bytes must be independent of total output")
    }

    func testStandardErrorIsCappedIndependentlyAndFlagsTruncation() throws {
        let cap = 16 * 1024
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: ["-c", "dd if=/dev/zero bs=1048576 count=4 2>/dev/null | tr '\\000' E >&2"],
            timeoutSeconds: 10,
            environment: Self.path,
            maximumOutputBytes: cap
        )

        XCTAssertEqual(result.standardError.count, cap)
        XCTAssertTrue(result.standardOutput.isEmpty)
        XCTAssertTrue(result.truncated)
    }

    /// Output that fits must be untouched and must not be flagged.
    func testOutputBelowTheCapIsCompleteAndNotFlagged() throws {
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: ["-c", "printf 'symaira'"],
            timeoutSeconds: 3,
            environment: Self.path,
            maximumOutputBytes: 1024
        )

        XCTAssertEqual(result.output, "symaira")
        XCTAssertFalse(result.truncated)
    }

    /// The default keeps existing call sites working unchanged: none of them
    /// passes a budget, and the shipped callers stay far below it.
    func testDefaultBudgetLeavesExistingCallSitesUnchanged() throws {
        XCTAssertEqual(BoundedProcessRunner.defaultMaximumOutputBytes, 8 * 1024 * 1024)

        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: writer(mebibytes: 2),
            timeoutSeconds: 10,
            environment: Self.path
        )

        XCTAssertEqual(result.standardOutput.count, 2 * 1024 * 1024)
        XCTAssertFalse(result.truncated, "2 MiB is inside the 8 MiB default")
    }

    func testAsyncAPIForwardsTheBudgetAndReportsTruncation() async throws {
        let cap = 8 * 1024
        let result = try await BoundedProcessRunner.runAsync(
            executable: "/bin/sh",
            arguments: writer(mebibytes: 2),
            timeoutSeconds: 10,
            environment: Self.path,
            maximumOutputBytes: cap
        )

        XCTAssertEqual(result.standardOutput.count, cap)
        XCTAssertTrue(result.truncated)
    }
}
