import XCTest

final class CLITests: XCTestCase {
    private var binary: URL!

    override func setUp() {
        super.setUp()
        let repoRoot = URL(fileURLWithPath: #file)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        binary = repoRoot
            .appendingPathComponent(".build")
            .appendingPathComponent("debug")
            .appendingPathComponent("symoperate")
    }

    // MARK: - Helpers

    private struct RunResult {
        let stdout: String
        let stderr: String
        let status: Int32
    }

    private func run(_ arguments: [String]) -> RunResult {
        let process = Process()
        process.executableURL = binary
        process.arguments = arguments

        let outPipe = Pipe()
        let errPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = errPipe

        try? process.run()
        process.waitUntilExit()

        let outData = outPipe.fileHandleForReading.readDataToEndOfFile()
        let errData = errPipe.fileHandleForReading.readDataToEndOfFile()
        return RunResult(
            stdout: String(data: outData, encoding: .utf8) ?? "",
            stderr: String(data: errData, encoding: .utf8) ?? "",
            status: process.terminationStatus
        )
    }

    // MARK: - --help / -h

    func testHelpFlagExitsZero() {
        let r = run(["--help"])
        XCTAssertEqual(r.status, 0, "--help should exit 0")
        XCTAssertTrue(r.stdout.contains("symoperate"), "--help should print usage")
        XCTAssertTrue(r.stdout.contains("Commands:"), "--help should list commands")
    }

    func testShortHelpFlagExitsZero() {
        let r = run(["-h"])
        XCTAssertEqual(r.status, 0, "-h should exit 0")
        XCTAssertTrue(r.stdout.contains("symoperate"), "-h should print usage")
        XCTAssertTrue(r.stdout.contains("Commands:"), "-h should list commands")
    }

    func testNoArgsExitsZero() {
        let r = run([])
        XCTAssertEqual(r.status, 0, "No args should exit 0")
        XCTAssertTrue(r.stdout.contains("symoperate"), "No args should print usage")
    }

    // MARK: - permissions grant JSON output

    func testPermissionsGrantMissingTargetFailsToStderr() {
        let r = run(["permissions", "grant"])
        XCTAssertNotEqual(r.status, 0, "permissions grant without target should fail")
        XCTAssertTrue(r.stderr.contains("error"), "Error should go to stderr")
        XCTAssertTrue(r.stderr.contains("requires"), "Error should explain the requirement")
    }

    func testPermissionsGrantInvalidTargetFailsToStderr() {
        let r = run(["permissions", "grant", "bogus"])
        XCTAssertNotEqual(r.status, 0, "permissions grant with invalid target should fail")
        XCTAssertTrue(r.stderr.contains("error"), "Error should go to stderr")
        XCTAssertTrue(r.stderr.contains("Unknown"), "Error should mention unknown target")
    }

    func testPermissionsGrantEmitsValidJSON() {
        // "screen" will either prompt or report already granted — either way, output must be valid JSON.
        let r = run(["permissions", "grant", "screen"])
        guard let data = r.stdout.data(using: .utf8) else {
            XCTFail("No stdout from permissions grant screen")
            return
        }
        let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        XCTAssertNotNil(json, "permissions grant should emit valid JSON to stdout")
        XCTAssertNotNil(json?["prompted"], "JSON should contain 'prompted' key")
    }
}
