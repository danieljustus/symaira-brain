import XCTest
@testable import SymOperateCore

final class SmokeTests: XCTestCase {
    private let controller = AutomationController(
        history: HistoryService(fileURL: FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-smoke-\(UUID().uuidString).jsonl"))
    )

    // MARK: - Snapshot

    func testSnapshotReturnsValidImageOrPermissionDenied() throws {
        do {
            let snapshot = try controller.snapshot()
            XCTAssertFalse(snapshot.id.isEmpty)
            XCTAssertFalse(snapshot.imageBase64PNG.isEmpty)
            XCTAssertGreaterThan(snapshot.imageSize.width, 0)
            XCTAssertGreaterThan(snapshot.imageSize.height, 0)
            XCTAssertGreaterThan(snapshot.displayBounds.width, 0)
            XCTAssertGreaterThan(snapshot.displayBounds.height, 0)
        } catch let error as AutomationError {
            if case .permissionDenied = error {
                XCTAssertTrue(error.localizedDescription.lowercased().contains("screen") || error.localizedDescription.lowercased().contains("recording"))
            } else if case .unavailable(let message) = error,
                      message.contains("not found in ScreenCaptureKit") {
                // Host with no ScreenCaptureKit-visible display (headless,
                // clamshell, or a stale TCC identity): CoreGraphics may still
                // report a display and both permissions may be granted, but
                // SCK exposes no capturable display, so capture cannot run.
            } else {
                XCTFail("Unexpected error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Query UI

    func testQueryUIReturnsTreeOrPermissionDenied() throws {
        do {
            let query = try controller.queryUI(maxDepth: 2, maxNodes: 50)
            XCTAssertFalse(query.snapshot.id.isEmpty)
            XCTAssertNotNil(query.app)
            XCTAssertGreaterThanOrEqual(query.nodes.count, 0)
        } catch let error as AutomationError {
            switch error {
            case .permissionDenied, .operationFailed, .unavailable, .invalidArgument:
                break
            default:
                XCTFail("Unexpected error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Click

    func testClickDoesNotCrash() throws {
        do {
            _ = try controller.click(x: 10, y: 10)
        } catch let error as AutomationError {
            switch error {
            case .permissionDenied, .operationFailed, .unavailable:
                break
            default:
                XCTFail("Unexpected error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Type text

    func testTypeTextDoesNotCrash() throws {
        do {
            _ = try controller.typeText("hello")
        } catch let error as AutomationError {
            switch error {
            case .permissionDenied, .operationFailed, .unavailable:
                break
            default:
                XCTFail("Unexpected error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Press keys

    func testPressKeysDoesNotCrash() throws {
        do {
            _ = try controller.pressKeys(["return"])
        } catch let error as AutomationError {
            switch error {
            case .permissionDenied, .operationFailed, .unavailable, .invalidArgument:
                break
            default:
                XCTFail("Unexpected error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: - Wait for

    func testWaitForThrowsWhenConditionNotMet() async throws {
        do {
            _ = try await controller.waitFor(text: "XYZ_NONEXISTENT_123", app: nil, timeoutSeconds: 1)
            XCTFail("Expected AutomationError to be thrown")
        } catch let autoError as AutomationError {
            if case .operationFailed = autoError {
                XCTAssertTrue(autoError.localizedDescription.contains("1 seconds") || autoError.localizedDescription.contains("within"))
            } else {
                XCTFail("Expected operationFailed, got \(autoError)")
            }
        } catch {
            XCTFail("Expected AutomationError, got \(type(of: error))")
        }
    }

    // MARK: - Doctor

    func testDoctorOutputsContainCapabilities() throws {
        let repoRoot = URL(fileURLWithPath: #file)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let binary = repoRoot
            .appendingPathComponent(".build")
            .appendingPathComponent("debug")
            .appendingPathComponent("symoperate")
        let process = Process()
        process.executableURL = binary
        process.arguments = ["doctor"]

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()

        try process.run()
        process.waitUntilExit()

        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            XCTFail("doctor did not emit valid JSON")
            return
        }

        XCTAssertNotNil(json["ok"], "Expected ok in doctor output")
        XCTAssertNotNil(json["capabilities"], "Expected capabilities in doctor output")
        XCTAssertNotNil(json["permissions"], "Expected permissions in doctor output")
        XCTAssertNotNil(json["recommendations"], "Expected recommendations in doctor output")

        if let capabilities = json["capabilities"] as? [String: Bool] {
            XCTAssertNotNil(capabilities["screenshot"])
            XCTAssertNotNil(capabilities["accessibility"])
        }
    }
}
