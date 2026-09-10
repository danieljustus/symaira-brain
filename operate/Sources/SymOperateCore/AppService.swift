import AppKit
import CoreGraphics
import Foundation

public struct AppService: AppServiceProtocol {
    public init() {}

    public func listApps() -> [AppInfo] {
        let frontmostPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
        return NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular }
            .sorted { ($0.localizedName ?? "") < ($1.localizedName ?? "") }
            .map {
                AppInfo(
                    localizedName: $0.localizedName ?? "Unknown",
                    bundleIdentifier: $0.bundleIdentifier,
                    processIdentifier: $0.processIdentifier,
                    isActive: $0.processIdentifier == frontmostPID
                )
            }
    }

    public func listWindows() -> [WindowInfo] {
        guard let rawList = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
            return []
        }

        return rawList.compactMap { row in
            guard
                let ownerName = row[kCGWindowOwnerName as String] as? String,
                let windowID = row[kCGWindowNumber as String] as? Int,
                let ownerPID = row[kCGWindowOwnerPID as String] as? Int32,
                let layer = row[kCGWindowLayer as String] as? Int,
                let bounds = row[kCGWindowBounds as String] as? [String: CGFloat],
                let x = bounds["X"],
                let y = bounds["Y"],
                let width = bounds["Width"],
                let height = bounds["Height"]
            else {
                return nil
            }

            let title = row[kCGWindowName as String] as? String
            return WindowInfo(
                windowID: windowID,
                ownerName: ownerName,
                ownerPID: ownerPID,
                title: title,
                bounds: RectValue(x: x, y: y, width: width, height: height),
                layer: layer
            )
        }
    }

    public func launchApp(bundleID: String? = nil, appName: String? = nil) throws {
        if let bundleID, !bundleID.isEmpty {
            if let appURL = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) {
                let configuration = NSWorkspace.OpenConfiguration()
                NSWorkspace.shared.openApplication(at: appURL, configuration: configuration)
                return
            }
            throw AutomationError.notFound("No application found for bundle identifier \(bundleID).")
        }

        if let appName, !appName.isEmpty {
            guard let result = try? BoundedAppProcessRunner.run(
                executable: "/usr/bin/open",
                arguments: ["-a", appName],
                timeoutSeconds: 10
            ), !result.timedOut, result.terminationStatus == 0 else {
                throw AutomationError.notFound("No application found named \(appName).")
            }
            return
        }

        throw AutomationError.invalidArgument("launch_app requires either bundle_id or app_name.")
    }

    public func focusWindow(bundleID: String? = nil, appName: String? = nil, title: String? = nil) throws {
        guard let app = resolveApp(bundleID: bundleID, appName: appName) else {
            throw AutomationError.notFound("No running application matched the requested app.")
        }

        app.activate(options: [.activateAllWindows])

        guard let title, !title.isEmpty else { return }

        let axApp = AXUIElementCreateApplication(app.processIdentifier)
        guard let windows = axCopyElements(axApp, attribute: kAXWindowsAttribute) else {
            throw AutomationError.notFound("The target application has no accessible windows.")
        }

        for window in windows {
            if let windowTitle = axCopyString(window, attribute: kAXTitleAttribute), windowTitle.localizedCaseInsensitiveContains(title) {
                _ = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
                return
            }
        }

        throw AutomationError.notFound("No window title matched '\(title)'.")
    }

    public func frontmostApp() -> AppInfo? {
        guard let app = NSWorkspace.shared.frontmostApplication else { return nil }
        return AppInfo(
            localizedName: app.localizedName ?? "Unknown",
            bundleIdentifier: app.bundleIdentifier,
            processIdentifier: app.processIdentifier,
            isActive: true
        )
    }

    private func resolveApp(bundleID: String?, appName: String?) -> NSRunningApplication? {
        if let bundleID, !bundleID.isEmpty {
            return NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).first
        }

        if let appName, !appName.isEmpty {
            return NSWorkspace.shared.runningApplications.first {
                ($0.localizedName ?? "").localizedCaseInsensitiveCompare(appName) == .orderedSame
            }
        }

        return NSWorkspace.shared.frontmostApplication
    }
}
