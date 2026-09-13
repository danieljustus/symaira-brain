import ApplicationServices
import AppKit
import CoreGraphics
import Darwin
import Foundation

/// The bounded native probes used by `PermissionService`.
///
/// Keeping the OS calls behind this adapter lets tests exercise the real
/// permission/request flow without prompting for TCC permissions or opening
/// System Settings. The production default remains the native macOS adapter.
public protocol PermissionProbeAdapter {
    func accessibilityTrusted(prompt: Bool) -> Bool
    func screenCapturePreflight() -> Bool
    func requestScreenCapture() -> Bool
    func openPrivacyPane(_ path: String) -> Bool
}

private struct NativePermissionProbeAdapter: PermissionProbeAdapter {
    func accessibilityTrusted(prompt: Bool) -> Bool {
        if prompt {
            let options = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
            return AXIsProcessTrustedWithOptions(options)
        }
        return AXIsProcessTrusted()
    }

    func screenCapturePreflight() -> Bool {
        CGPreflightScreenCaptureAccess()
    }

    func requestScreenCapture() -> Bool {
        CGRequestScreenCaptureAccess()
    }

    func openPrivacyPane(_ path: String) -> Bool {
        let url = URL(string: "x-apple.systempreferences:\(path)")
        if let url, NSWorkspace.shared.open(url) { return true }
        // Pane-specific deep links can be rejected by newer macOS builds; the
        // generic Privacy pane always opens.
        if let fallback = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy") {
            return NSWorkspace.shared.open(fallback)
        }
        return false
    }
}

public struct PermissionService: PermissionServiceProtocol {
    private let probe: any PermissionProbeAdapter

    public init(probe: (any PermissionProbeAdapter)? = nil) {
        self.probe = probe ?? NativePermissionProbeAdapter()
    }

    public func status() -> PermissionSnapshot {
        let pid = getpid()
        let ppid = getppid()
        let execPath = executablePath(for: pid) ?? ProcessInfo.processInfo.arguments.first ?? "unknown"
        let parentName = processName(for: ppid)

        return PermissionSnapshot(
            accessibilityGranted: probe.accessibilityTrusted(prompt: false),
            screenRecordingGranted: probe.screenCapturePreflight(),
            source: PermissionSource(
                pid: pid,
                ppid: ppid,
                executablePath: execPath,
                launchingProcessName: parentName,
                note: "These booleans describe the TCC grants held by the process identified above. On macOS, TCC permissions (Accessibility, Screen Recording) are per-process — granting them to the launching app (e.g., Terminal, Cursor) does NOT make them available to the MCP host that launched symoperate. Grant permissions from the process that will actually be using symoperate's MCP server."
            )
        )
    }

    @discardableResult
    public func requestAccessibilityPermission() -> Bool {
        if probe.accessibilityTrusted(prompt: true) { return true }
        // The prompt fires only once per app identity; afterwards this call is
        // silent and the user needs to reach the pane themselves.
        _ = probe.openPrivacyPane("com.apple.preference.security?Privacy_Accessibility")
        return probe.accessibilityTrusted(prompt: false)
    }

    @discardableResult
    public func requestScreenRecordingPermission() -> Bool {
        if probe.screenCapturePreflight() || probe.requestScreenCapture() { return true }
        _ = probe.openPrivacyPane("com.apple.preference.security?Privacy_ScreenCapture")
        return probe.screenCapturePreflight()
    }

    // MARK: - Process info helpers

    /// Maximum buffer size for `proc_pidpath` as defined in `<libproc.h>`.
    private static let procPathBufferSize: UInt32 = 4096

    /// Returns the resolved absolute executable path for the given PID, or nil on failure.
    private func executablePath(for pid: pid_t) -> String? {
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: Int(Self.procPathBufferSize))
        defer { buffer.deallocate() }
        let written = proc_pidpath(pid, buffer, Self.procPathBufferSize)
        guard written > 0 else { return nil }
        return String(cString: buffer)
    }

    /// Returns a human-readable process name for the given PID, or nil if unobtainable.
    /// On macOS this uses `proc_name()` from libproc, which works for both GUI apps and
    /// command-line processes.
    private func processName(for pid: pid_t) -> String? {
        var buffer = [CChar](repeating: 0, count: Int(MAXCOMLEN) + 1)
        let written = proc_name(pid, &buffer, UInt32(MAXCOMLEN))
        guard written > 0 else { return nil }
        let name = buffer.withUnsafeBytes { rawBuf in
            String(bytes: rawBuf.prefix(Int(MAXCOMLEN)), encoding: .utf8)
        }
        .map { $0.trimmingCharacters(in: .controlCharacters).trimmingCharacters(in: .whitespaces) } ?? nil
        return name?.isEmpty == false ? name : nil
    }
}
