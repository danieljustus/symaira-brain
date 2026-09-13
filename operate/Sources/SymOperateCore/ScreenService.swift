import AppKit
import CoreGraphics
import CoreMedia
import Foundation
import ImageIO
@preconcurrency import ScreenCaptureKit

public final class ScreenService: ScreenServiceProtocol {
    private let fm = FileManager.default
    private let snapshotDirectory: URL

    public init(snapshotDirectory: URL = FileManager.default.temporaryDirectory.appendingPathComponent("symoperate-snapshots", isDirectory: true)) {
        self.snapshotDirectory = snapshotDirectory
        try? fm.createDirectory(
            at: snapshotDirectory,
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        try? fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: snapshotDirectory.path)
        cleanupOldSnapshots()
    }

    /// Deletes snapshot PNGs older than 5 minutes to prevent unbounded disk accumulation.
    private func cleanupOldSnapshots() {
        let cutoff = Date().addingTimeInterval(-300) // 5 minutes
        guard let files = try? fm.contentsOfDirectory(at: snapshotDirectory, includingPropertiesForKeys: [.creationDateKey]) else { return }
        for file in files where file.pathExtension == "png" {
            guard let attrs = try? fm.attributesOfItem(atPath: file.path),
                  let created = attrs[.creationDate] as? Date,
                  created < cutoff else { continue }
            try? fm.removeItem(at: file)
        }
    }

    public func listDisplays() -> [DisplayInfo] {
        var displayCount: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &displayCount) == .success, displayCount > 0 else {
            let mainID = CGMainDisplayID()
            let mainBounds = CGDisplayBounds(mainID)
            return [DisplayInfo(displayID: mainID, bounds: RectValue(x: mainBounds.origin.x, y: mainBounds.origin.y, width: mainBounds.size.width, height: mainBounds.size.height), isMain: true)]
        }

        var ids = [CGDirectDisplayID](repeating: 0, count: Int(displayCount))
        _ = CGGetActiveDisplayList(displayCount, &ids, &displayCount)

        let mainID = CGMainDisplayID()
        var seen = Set<UInt32>()
        var displays: [DisplayInfo] = []

        for id in ids {
            guard !seen.contains(id) else { continue }
            seen.insert(id)
            let bounds = CGDisplayBounds(id)
            displays.append(DisplayInfo(
                displayID: id,
                bounds: RectValue(x: bounds.origin.x, y: bounds.origin.y, width: bounds.size.width, height: bounds.size.height),
                isMain: id == mainID
            ))
        }

        return displays
    }

    public func captureMainDisplay(maxDimension: CGFloat) throws -> Snapshot {
        try capture(
            displayID: CGMainDisplayID(),
            bounds: CGDisplayBounds(CGMainDisplayID()),
            maxDimension: maxDimension,
            saveDebugImage: false
        )
    }

    public func captureDisplay(displayID: UInt32, maxDimension: CGFloat) throws -> Snapshot {
        try capture(
            displayID: displayID,
            bounds: CGDisplayBounds(displayID),
            maxDimension: maxDimension,
            saveDebugImage: false
        )
    }

    public func captureWindow(windowID: Int, maxDimension: CGFloat) throws -> Snapshot {
        guard let target = windowInfo(for: windowID) else {
            throw AutomationError.notFound("Window \(windowID) was not found.")
        }
        let windowBounds = CGRect(
            x: target.bounds.x,
            y: target.bounds.y,
            width: target.bounds.width,
            height: target.bounds.height
        )
        return try capture(
            // Report the display the window actually sits on. The capture no
            // longer depends on it, but a snapshot that always claimed the main
            // display was wrong for every window on a secondary screen.
            displayID: Self.displayContaining(windowBounds),
            bounds: windowBounds,
            maxDimension: maxDimension,
            windowID: windowID,
            windowOwnerPID: target.ownerPID,
            saveDebugImage: false
        )
    }

    private func capture(
        displayID: CGDirectDisplayID,
        bounds: CGRect,
        maxDimension: CGFloat,
        windowID: Int? = nil,
        windowOwnerPID: Int32? = nil,
        saveDebugImage: Bool = false
    ) throws -> Snapshot {
        let id = UUID().uuidString

        let captureResult: (image: CGImage, contentRect: CGRect)
        if let windowID {
            captureResult = try captureScreenWithScreenCaptureKit(windowID: windowID)
        } else {
            captureResult = try captureScreenWithScreenCaptureKit(displayID: displayID)
        }

        let scaled = resizeIfNeeded(image: captureResult.image, maxDimension: maxDimension)
        let png = try pngData(for: scaled)

        var debugPath: String?
        if saveDebugImage {
            debugPath = try writeDebugPNG(png, id: id).path
        }

        cleanupOldSnapshots()

        // A window capture returns an image of the window alone, so the rect the
        // snapshot advertises — and that `SnapshotTransform` maps image points
        // back into — must be the window's frame rather than the display.
        let mappedBounds = Self.snapshotBounds(
            requested: bounds,
            capturedWindowFrame: windowID == nil ? nil : captureResult.contentRect
        )

        let imageSize = SizeValue(width: Double(scaled.width), height: Double(scaled.height))
        let rectValue = RectValue(
            x: mappedBounds.origin.x,
            y: mappedBounds.origin.y,
            width: mappedBounds.size.width,
            height: mappedBounds.size.height
        )
        let transform = SnapshotTransform(displayID: displayID, displayBounds: rectValue, imageSize: imageSize)
        return Snapshot(
            id: id,
            createdAt: DateFormats.iso8601String(from: Date()),
            imageBase64PNG: png.base64EncodedString(),
            imageSize: imageSize,
            displayBounds: rectValue,
            displayID: displayID,
            windowID: windowID,
            windowOwnerPID: windowOwnerPID,
            debugImagePath: debugPath,
            transform: transform
        )
    }

    /// The screen rectangle a snapshot describes.
    ///
    /// For a display capture this is the requested display rect. For a window
    /// capture it is the frame of the window that was actually captured, so the
    /// advertised rect and the returned image cover the same area and
    /// ``SnapshotTransform`` maps between them correctly. A zero frame — which
    /// ScreenCaptureKit can hand back — would collapse every mapped coordinate
    /// onto one point, so the caller's resolved bounds stay the safer answer.
    ///
    /// Internal rather than private so the geometry can be tested without a
    /// Screen Recording grant.
    static func snapshotBounds(requested: CGRect, capturedWindowFrame: CGRect?) -> CGRect {
        guard let capturedWindowFrame, !capturedWindowFrame.isEmpty else { return requested }
        return capturedWindowFrame
    }

    /// The display whose bounds contain `rect`, falling back to the main display
    /// when the lookup fails or the rect is off-screen.
    static func displayContaining(_ rect: CGRect) -> CGDirectDisplayID {
        var matching: UInt32 = 0
        guard CGGetDisplaysWithRect(rect, 0, nil, &matching) == .success, matching > 0 else {
            return CGMainDisplayID()
        }
        var ids = [CGDirectDisplayID](repeating: 0, count: Int(matching))
        guard CGGetDisplaysWithRect(rect, matching, &ids, &matching) == .success,
              let first = ids.first, matching > 0 else {
            return CGMainDisplayID()
        }
        return first
    }

    @discardableResult
    func writeDebugPNG(_ png: Data, id: String) throws -> URL {
        let path = snapshotDirectory.appendingPathComponent("\(id).png")
        try png.write(to: path, options: .atomic)
        try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
        return path
    }

    private func captureScreenWithScreenCaptureKit(displayID: CGDirectDisplayID = CGMainDisplayID()) throws -> (image: CGImage, contentRect: CGRect) {
        let box = SendableBox<CGImage?>(value: nil)
        let rectBox = SendableBox<CGRect?>(value: nil)
        let errorBox = SendableBox<Error?>(value: nil)
        let semaphore = DispatchSemaphore(value: 0)

        Task { @MainActor in
            do {
                let content = try await SCShareableContent.current

                guard let display = content.displays.first(where: { $0.displayID == displayID }) else {
                    throw AutomationError.unavailable("Display \(displayID) not found in ScreenCaptureKit.")
                }

                let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
                let config = SCStreamConfiguration()
                config.pixelFormat = kCVPixelFormatType_32BGRA
                config.showsCursor = false

                let (image, rect) = try await SCScreenshotManager.captureImage(
                    contentFilter: filter,
                    configuration: config
                )
                box.value = image
                rectBox.value = rect
            } catch {
                errorBox.value = error
            }
            semaphore.signal()
        }

        if Thread.isMainThread {
            while semaphore.wait(timeout: .now()) == .timedOut {
                RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
            }
        } else {
            semaphore.wait()
        }

        if let error = errorBox.value {
            throw Self.classifyCaptureError(error, context: "Screen capture")
        }

        guard let image = box.value else {
            throw AutomationError.operationFailed("Failed to capture screen image.")
        }

        return (image, rectBox.value ?? CGRect.zero)
    }

    private func captureScreenWithScreenCaptureKit(windowID: Int) throws -> (image: CGImage, contentRect: CGRect) {
        let box = SendableBox<CGImage?>(value: nil)
        let rectBox = SendableBox<CGRect?>(value: nil)
        let errorBox = SendableBox<Error?>(value: nil)
        let semaphore = DispatchSemaphore(value: 0)

        Task { @MainActor in
            do {
                let content = try await SCShareableContent.current

                guard let window = content.windows.first(where: { $0.windowID == windowID }) else {
                    throw AutomationError.notFound("Window \(windowID) not found in ScreenCaptureKit.")
                }

                // Isolate the window itself. The display-based filter this used
                // to build captured the *whole* display instead: with an empty
                // `excludingApplications` list nothing is excluded, and
                // `exceptingWindows` only re-includes windows whose owning
                // application appears in that exclusion list — so naming the
                // target window there had no effect at all. Scoping a capture to
                // one window is the only control this API offers over what
                // leaves the machine, and it silently returned every other
                // application's window plus the desktop and dock.
                //
                // `desktopIndependentWindow` also makes the capture independent
                // of which display the window is on, which removes the previous
                // hardcoded main-display lookup.
                let filter = SCContentFilter(desktopIndependentWindow: window)
                let config = SCStreamConfiguration()
                config.pixelFormat = kCVPixelFormatType_32BGRA
                config.showsCursor = false

                let (image, _) = try await SCScreenshotManager.captureImage(
                    contentFilter: filter,
                    configuration: config
                )
                box.value = image
                // The window's frame in screen points — the rectangle the
                // captured image actually covers. The sample buffer's own
                // content rect is in pixel coordinates within the captured
                // surface, so it cannot be mapped back to the screen.
                rectBox.value = window.frame
            } catch {
                errorBox.value = error
            }
            semaphore.signal()
        }

        if Thread.isMainThread {
            while semaphore.wait(timeout: .now()) == .timedOut {
                RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
            }
        } else {
            semaphore.wait()
        }

        if let error = errorBox.value {
            throw Self.classifyCaptureError(error, context: "Window capture")
        }

        guard let image = box.value else {
            throw AutomationError.operationFailed("Failed to capture window image.")
        }

        return (image, rectBox.value ?? CGRect.zero)
    }

    /// Maps a ScreenCaptureKit capture failure to the appropriate `AutomationError`.
    ///
    /// Screen Recording TCC denials are classified as `.permissionDenied` so MCP
    /// clients receive a classified refusal instead of a generic `operationFailed`.
    /// Detection covers the ScreenCaptureKit `-3801` code, the `com.apple.TCC`
    /// error domain, and localized denial messages (e.g. German
    /// "Benutzer:in hat TCCs für die Aufnahme durch Apps, Fenster, Displays abgelehnt").
    ///
    /// Typed `AutomationError`s thrown by the capture paths (display mismatch →
    /// `.unavailable`, missing window → `.notFound`) are passed through unchanged;
    /// re-wrapping them as `.operationFailed` would hide the distinct
    /// environment/not-found states from MCP clients and `doctor`.
    static func classifyCaptureError(_ error: Error, context: String) -> AutomationError {
        if let automationError = error as? AutomationError {
            return automationError
        }
        if Self.isScreenRecordingPermissionDenied(error) {
            return AutomationError.permissionDenied(
                "Screen Recording permission is denied. Enable it in System Settings > Privacy & Security > Screen Recording."
            )
        }
        return AutomationError.operationFailed("\(context) failed: \(error.localizedDescription)")
    }

    private static func isScreenRecordingPermissionDenied(_ error: Error) -> Bool {
        let nsError = error as NSError
        if nsError.domain == "com.apple.ScreenCaptureKit", nsError.code == -3801 {
            return true
        }
        if nsError.domain == "com.apple.TCC" {
            return true
        }
        let message = error.localizedDescription.lowercased()
        return message.contains("denied") || message.contains("abgelehnt")
    }

    private func windowInfo(for windowID: Int) -> WindowInfo? {
        guard let rawList = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
            return nil
        }

        for row in rawList {
            guard
                let ownerName = row[kCGWindowOwnerName as String] as? String,
                let wid = row[kCGWindowNumber as String] as? Int, wid == windowID,
                let ownerPID = row[kCGWindowOwnerPID as String] as? Int32,
                let layer = row[kCGWindowLayer as String] as? Int,
                let bounds = row[kCGWindowBounds as String] as? [String: CGFloat],
                let x = bounds["X"],
                let y = bounds["Y"],
                let width = bounds["Width"],
                let height = bounds["Height"]
            else { continue }

            return WindowInfo(
                windowID: wid,
                ownerName: ownerName,
                ownerPID: ownerPID,
                title: row[kCGWindowName as String] as? String,
                bounds: RectValue(x: x, y: y, width: width, height: height),
                layer: layer
            )
        }

        return nil
    }

    private func resizeIfNeeded(image: CGImage, maxDimension: CGFloat) -> CGImage {
        let width = CGFloat(image.width)
        let height = CGFloat(image.height)
        let ratio = min(1.0, maxDimension / max(width, height))
        guard ratio < 0.999 else { return image }

        let targetWidth = Int((width * ratio).rounded(.toNearestOrEven))
        let targetHeight = Int((height * ratio).rounded(.toNearestOrEven))

        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue
        guard let context = CGContext(
            data: nil,
            width: targetWidth,
            height: targetHeight,
            bitsPerComponent: 8,
            bytesPerRow: targetWidth * 4,
            space: colorSpace,
            bitmapInfo: bitmapInfo
        ) else {
            return image
        }

        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: CGFloat(targetWidth), height: CGFloat(targetHeight)))
        return context.makeImage() ?? image
    }

    private func pngData(for image: CGImage) throws -> Data {
        let rep = NSBitmapImageRep(cgImage: image)
        guard let data = rep.representation(using: .png, properties: [:]) else {
            throw AutomationError.operationFailed("Failed to encode screenshot as PNG.")
        }
        return data
    }
}

private final class SendableBox<T>: @unchecked Sendable {
    var value: T
    init(value: T) {
        self.value = value
    }
}

private extension SCScreenshotManager {
    static func captureImage(
        contentFilter filter: SCContentFilter,
        configuration config: SCStreamConfiguration
    ) async throws -> (CGImage, CGRect) {
        let sampleBuffer = try await captureSampleBuffer(
            contentFilter: filter,
            configuration: config
        )

        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
            throw AutomationError.operationFailed("No pixel buffer in sample buffer.")
        }

        let ciImage = CIImage(cvPixelBuffer: pixelBuffer)

        guard let cgImage = CIContext().createCGImage(
            ciImage,
            from: ciImage.extent
        ) else {
            throw AutomationError.operationFailed("Failed to create CGImage from pixel buffer.")
        }

        var contentRect = CGRect.zero
        if let attachmentsArray = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[String: Any]] {
            if let firstAttachment = attachmentsArray.first {
                if let rectDict = firstAttachment["Rect"] as? [String: CGFloat] {
                    contentRect = CGRect(
                        x: rectDict["X"] ?? 0,
                        y: rectDict["Y"] ?? 0,
                        width: rectDict["Width"] ?? 0,
                        height: rectDict["Height"] ?? 0
                    )
                }
            }
        }

        return (cgImage, contentRect)
    }
}
