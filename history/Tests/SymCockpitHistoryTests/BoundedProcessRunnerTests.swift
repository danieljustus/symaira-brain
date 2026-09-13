import Foundation
import XCTest
@testable import SymCockpitHistory

final class BoundedProcessRunnerTests: XCTestCase {
    func testResolvesExecutableThroughProvidedPATHAndCapturesBothStreams() throws {
        let result = try BoundedProcessRunner.run(
            executable: "sh",
            arguments: ["-c", "printf out; printf err >&2"],
            timeoutSeconds: 1,
            environment: ["PATH": "/bin:/usr/bin"]
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(String(data: result.standardOutput, encoding: .utf8), "out")
        XCTAssertEqual(String(data: result.standardError, encoding: .utf8), "err")
    }

    func testDrainsMoreThanOneMiBFromBothStreamsWithoutCorruption() throws {
        let bytesPerStream = 2 * 1024 * 1024
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: [
                "-c",
                "dd if=/dev/zero bs=1024 count=2048 2>/dev/null | tr '\\000' O; " +
                    "dd if=/dev/zero bs=1024 count=2048 2>/dev/null | tr '\\000' E >&2"
            ],
            timeoutSeconds: 3,
            environment: ["PATH": "/bin:/usr/bin"]
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(result.standardOutput.count, bytesPerStream)
        XCTAssertEqual(result.standardError.count, bytesPerStream)
        XCTAssertTrue(result.standardOutput.allSatisfy { $0 == 0x4F })
        XCTAssertTrue(result.standardError.allSatisfy { $0 == 0x45 })
    }

    func testWritesOptionalStandardInputAndClosesItAtEOF() throws {
        let result = try BoundedProcessRunner.run(
            executable: "cat",
            timeoutSeconds: 1,
            environment: ["PATH": "/bin:/usr/bin"],
            standardInput: Data("from stdin".utf8)
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(result.output, "from stdin")
    }

    func testTimeoutTerminatesAndReapsTheChild() throws {
        let started = Date()
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sh",
            arguments: ["-c", "trap '' TERM; sleep 10"],
            timeoutSeconds: 0.05
        )

        XCTAssertTrue(result.timedOut)
        XCTAssertLessThan(Date().timeIntervalSince(started), 1)
        XCTAssertNotEqual(result.terminationStatus, 0)
    }

    func testMissingPATHExecutableDoesNotFallBackToAUserSpecificPath() {
        XCTAssertThrowsError(try BoundedProcessRunner.run(
            executable: "definitely-not-a-real-process",
            environment: ["PATH": "/empty"]
        )) { error in
            XCTAssertEqual(
                error as? BoundedProcessRunnerError,
                .executableUnavailable("definitely-not-a-real-process")
            )
        }
    }

    /// Regression test for a GUI-launched process (launchd's minimal PATH,
    /// no Homebrew) still finding a sibling Symaira CLI installed at the
    /// documented managed location, ~/.symaira/bin.
    func testFallsBackToManagedSymairaBinDirectoryWhenPATHOmitsIt() throws {
        let tempHome = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let binDir = tempHome.appendingPathComponent(".symaira/bin")
        try FileManager.default.createDirectory(at: binDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempHome) }

        let scriptPath = binDir.appendingPathComponent("fixture-symtool")
        try "#!/bin/sh\nprintf found\n".write(to: scriptPath, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: scriptPath.path)

        let result = try BoundedProcessRunner.run(
            executable: "fixture-symtool",
            timeoutSeconds: 1,
            environment: ["PATH": "/empty", "HOME": tempHome.path]
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(result.output, "found")
    }
}
