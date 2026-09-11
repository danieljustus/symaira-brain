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

    // MARK: - serve stdio hygiene

    /// `serve` is the MCP entry point: stdout is the JSON-RPC transport and
    /// stderr is the host's diagnostic channel, so nothing unsolicited may
    /// land on either during a normal session — MCP hosts, and the smoke
    /// check that guards symoperate as vendored into symaira-brain, both
    /// treat a stray stderr byte here as a protocol violation. Regression
    /// coverage for the background update-check nag that used to violate
    /// this on every `serve` launch.
    func testServeWritesNoStderrDuringMCPInitialize() throws {
        let process = Process()
        process.executableURL = binary
        process.arguments = ["serve"]

        let inPipe = Pipe()
        let outPipe = Pipe()
        let errPipe = Pipe()
        process.standardInput = inPipe
        process.standardOutput = outPipe
        process.standardError = errPipe

        try process.run()

        // Safety net only: `run` doc-comments "serves MCP over stdio until
        // stdin closes", so closing stdin below is expected to end the
        // process on its own well before this fires.
        DispatchQueue.global().asyncAfter(deadline: .now() + 8) {
            if process.isRunning {
                process.terminate()
            }
        }

        let request = #"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cli-test","version":"1"}}}"# + "\n"
        inPipe.fileHandleForWriting.write(Data(request.utf8))

        // Keep stdin open past `initialize` so a still-running background
        // update check (a real, asynchronous GitHub API round trip, not
        // gated on the request/response exchange) has time to land before
        // the transport shuts down. Closing stdin immediately after writing
        // would let the process exit and race right past a regression here.
        Thread.sleep(forTimeInterval: 2.0)
        try? inPipe.fileHandleForWriting.close()

        let outData = outPipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let errData = errPipe.fileHandleForReading.readDataToEndOfFile()

        let stderrText = String(data: errData, encoding: .utf8) ?? ""
        XCTAssertTrue(stderrText.isEmpty, "serve must write nothing to stderr while handling MCP initialize, got: \(stderrText)")

        let stdoutText = String(data: outData, encoding: .utf8) ?? ""
        XCTAssertTrue(stdoutText.contains("\"id\":1"), "expected a JSON-RPC response to the initialize request on stdout")
    }
}
