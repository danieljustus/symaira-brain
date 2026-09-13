import XCTest
@testable import SymOperateCore

final class ScreenServiceTests: XCTestCase {
    func testSnapshotDirectoryAndDebugPNGUsePrivateModes() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-mode-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let service = ScreenService(snapshotDirectory: directory)
        let directoryAttributes = try FileManager.default.attributesOfItem(atPath: directory.path)
        XCTAssertEqual((directoryAttributes[.posixPermissions] as? NSNumber)?.intValue, 0o700)

        let debugPNG = try service.writeDebugPNG(
            Data([0x89, 0x50, 0x4E, 0x47]),
            id: "mode-regression"
        )
        let fileAttributes = try FileManager.default.attributesOfItem(atPath: debugPNG.path)
        XCTAssertEqual((fileAttributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)
    }

    // MARK: - Screen Recording TCC denial classification

    func testTCCDomainDenialClassifiesAsPermissionDenied() {
        let tccDenial = NSError(
            domain: "com.apple.TCC",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: "The user denied the screen capture."]
        )
        let classified = ScreenService.classifyCaptureError(tccDenial, context: "Screen capture")
        guard case .permissionDenied = classified else {
            return XCTFail("Expected .permissionDenied, got \(classified)")
        }
        XCTAssertEqual(
            classified.localizedDescription,
            "Screen Recording permission is denied. Enable it in System Settings > Privacy & Security > Screen Recording."
        )
    }

    func testScreenCaptureKit3801ClassifiesAsPermissionDenied() {
        let scDenial = NSError(
            domain: "com.apple.ScreenCaptureKit",
            code: -3801,
            userInfo: [NSLocalizedDescriptionKey: "The user declined the request."]
        )
        let classified = ScreenService.classifyCaptureError(scDenial, context: "Window capture")
        guard case .permissionDenied = classified else {
            return XCTFail("Expected .permissionDenied, got \(classified)")
        }
    }

    func testLocalizedTCCDenialMessageClassifiesAsPermissionDenied() {
        // ScreenCaptureKit surfaces TCC denials with localized messages that vary
        // by system language — e.g. German: "Benutzer:in hat TCCs für die Aufnahme
        // durch Apps, Fenster, Displays abgelehnt" — without a stable error code.
        let localizedDenial = NSError(
            domain: "com.apple.ScreenCaptureKit",
            code: -3802,
            userInfo: [
                NSLocalizedDescriptionKey: "Benutzer:in hat TCCs für die Aufnahme durch Apps, Fenster, Displays abgelehnt"
            ]
        )
        let classified = ScreenService.classifyCaptureError(localizedDenial, context: "Screen capture")
        guard case .permissionDenied = classified else {
            return XCTFail("Expected .permissionDenied, got \(classified)")
        }
    }

    func testUnrelatedCaptureErrorRemainsOperationFailed() {
        let generic = NSError(
            domain: "com.apple.ScreenCaptureKit",
            code: -9999,
            userInfo: [NSLocalizedDescriptionKey: "Stream error: no frames were received."]
        )
        let classified = ScreenService.classifyCaptureError(generic, context: "Screen capture")
        guard case .operationFailed(let message) = classified else {
            return XCTFail("Expected .operationFailed, got \(classified)")
        }
        XCTAssertTrue(message.contains("Screen capture failed"))
    }

    // MARK: - Typed error pass-through (issue #108)

    func testTypedUnavailableErrorPassesThroughClassifier() {
        // Display-mismatch errors thrown by the capture paths are already
        // classified; the classifier must not re-wrap them as operationFailed.
        let displayMismatch = AutomationError.unavailable("Display 1 not found in ScreenCaptureKit.")
        let classified = ScreenService.classifyCaptureError(displayMismatch, context: "Screen capture")
        guard case .unavailable(let message) = classified else {
            return XCTFail("Expected .unavailable to pass through, got \(classified)")
        }
        XCTAssertEqual(message, "Display 1 not found in ScreenCaptureKit.")
        XCTAssertEqual(classified.code, "element_not_resolvable")
    }

    func testTypedNotFoundErrorPassesThroughClassifier() {
        let windowMissing = AutomationError.notFound("Window 42 not found in ScreenCaptureKit.")
        let classified = ScreenService.classifyCaptureError(windowMissing, context: "Window capture")
        guard case .notFound(let message) = classified else {
            return XCTFail("Expected .notFound to pass through, got \(classified)")
        }
        XCTAssertEqual(message, "Window 42 not found in ScreenCaptureKit.")
    }

    // MARK: - Doctor advice (issue #108)

    func testDoctorAdviceForUnavailableCaptureErrorIsActionable() {
        let advice = DoctorAdvice.screenshotProbeRecommendation(
            for: .unavailable("Display 1 not found in ScreenCaptureKit.")
        )
        XCTAssertTrue(advice.contains("ScreenCaptureKit"), advice)
        XCTAssertTrue(advice.contains("display"), advice)
        XCTAssertTrue(advice.contains("Screen Recording"), advice)
    }

    func testDoctorAdviceForOtherErrorsReturnsLocalizedMessage() {
        let denied = AutomationError.permissionDenied("Screen Recording permission is denied. Enable it.")
        XCTAssertEqual(
            DoctorAdvice.screenshotProbeRecommendation(for: denied),
            "Screen Recording permission is denied. Enable it."
        )
        let failed = AutomationError.operationFailed("Screen capture failed: stream error")
        XCTAssertEqual(
            DoctorAdvice.screenshotProbeRecommendation(for: failed),
            "Screen capture failed: stream error"
        )
    }

    // MARK: - Window-scoped capture geometry (issue #240)

    func testWindowSnapshotDescribesTheCapturedWindowRectNotTheDisplay() {
        // A window capture returns an image of the window alone, so the rect the
        // snapshot advertises must be the window's own frame — not the display
        // rect the caller happened to pass in.
        let display = CGRect(x: 0, y: 0, width: 3456, height: 2234)
        let window = CGRect(x: 400, y: 220, width: 1200, height: 800)

        XCTAssertEqual(
            ScreenService.snapshotBounds(requested: display, capturedWindowFrame: window),
            window
        )
    }

    func testDisplaySnapshotKeepsTheRequestedDisplayBounds() {
        // The display path passes no window frame; its bounds stay untouched.
        let display = CGRect(x: 0, y: 0, width: 3456, height: 2234)

        XCTAssertEqual(
            ScreenService.snapshotBounds(requested: display, capturedWindowFrame: nil),
            display
        )
    }

    func testEmptyCapturedWindowFrameFallsBackToTheRequestedBounds() {
        // ScreenCaptureKit can hand back a zero frame; an empty rect would make
        // every mapped coordinate collapse onto a single point, so the caller's
        // resolved bounds remain the safer answer.
        let display = CGRect(x: 0, y: 0, width: 3456, height: 2234)

        XCTAssertEqual(
            ScreenService.snapshotBounds(requested: display, capturedWindowFrame: .zero),
            display
        )
    }

    func testTransformMapsImageCentreToWindowCentreForAWindowSnapshot() {
        // The acceptance check for #240: with the image and the advertised rect
        // both describing the window, the centre of the image maps back to the
        // centre of the window on screen. Before the fix the image covered the
        // whole display while the rect described the window, so this landed far
        // from the window's real centre.
        let display = CGRect(x: 0, y: 0, width: 3456, height: 2234)
        let window = CGRect(x: 400, y: 220, width: 1200, height: 800)

        let bounds = ScreenService.snapshotBounds(requested: display, capturedWindowFrame: window)
        let rectValue = RectValue(
            x: bounds.origin.x,
            y: bounds.origin.y,
            width: bounds.size.width,
            height: bounds.size.height
        )
        // A window-sized capture, downscaled by maxDimension the way capture() does.
        let imageSize = SizeValue(width: 1200 / 2, height: 800 / 2)
        let transform = SnapshotTransform(displayID: 1, displayBounds: rectValue, imageSize: imageSize)

        let centre = transform.imageToDisplay(
            point: PointValue(x: imageSize.width / 2, y: imageSize.height / 2)
        )

        XCTAssertEqual(centre.x, window.midX, accuracy: 0.0001)
        XCTAssertEqual(centre.y, window.midY, accuracy: 0.0001)
    }
}
