import Foundation
import FreeformSnapshotCore

public typealias FreeformHelperStatus = FreeformSnapshot.Status

public struct FreeformSnapshotResult: Codable, Sendable {
    public let ok: Bool
    public let snapshot_path: String?
    public let created_at: Double?
    public let error: String?
}

/// All launchd effects are injectable. Reading doctor status never invokes this seam.
public protocol FreeformSnapshotRunner {
    func kickstart(arguments: [String], timeout: TimeInterval) throws -> (status: Int32, timedOut: Bool)
}

public struct NativeFreeformSnapshotRunner: FreeformSnapshotRunner {
    public init() {}
    public func kickstart(arguments: [String], timeout: TimeInterval) throws -> (status: Int32, timedOut: Bool) {
        let result = try BoundedAppProcessRunner.run(executable: "/bin/launchctl", arguments: arguments, timeoutSeconds: timeout)
        return (result.terminationStatus, result.timedOut)
    }
}

public protocol FreeformSnapshotServiceProtocol {
    var helperInstalled: Bool { get }
    var lastStatus: FreeformHelperStatus? { get }
    func snapshot(timeout: TimeInterval) -> FreeformSnapshotResult
}

/// On-demand launchd job retains its own TCC identity. Never launch the helper as our child.
public final class FreeformSnapshotService: FreeformSnapshotServiceProtocol {
    private let directory: URL
    private let plist: URL
    private let runner: any FreeformSnapshotRunner
    private let uid: UInt32
    private let lock = NSLock()

    public init(directory: URL = FreeformSnapshot.directory,
                plist: URL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(
                    "Library/LaunchAgents/com.symaira.operate.freeform-snapshot.plist"),
                runner: any FreeformSnapshotRunner = NativeFreeformSnapshotRunner(), uid: UInt32 = getuid()) {
        self.directory = directory
        self.plist = plist
        self.runner = runner
        self.uid = uid
    }

    public var helperInstalled: Bool { FileManager.default.fileExists(atPath: plist.path) }
    public var lastStatus: FreeformHelperStatus? {
        let url = directory.appendingPathComponent("status.json")
        guard let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size <= 64 * 1024,
              let data = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder().decode(FreeformHelperStatus.self, from: data)
    }

    public func snapshot(timeout: TimeInterval = 15) -> FreeformSnapshotResult {
        lock.lock()
        defer { lock.unlock() }
        let start = Date().timeIntervalSince1970
        let deadline = ProcessInfo.processInfo.systemUptime + timeout
        func failure(_ message: String) -> FreeformSnapshotResult {
            FreeformSnapshotResult(ok: false, snapshot_path: nil, created_at: nil, error: message)
        }
        guard timeout.isFinite, timeout > 0, timeout <= 60 else { return failure("timeout must be > 0 and <= 60 seconds.") }
        guard helperInstalled else { return failure(installationAdvice) }
        do {
            let result = try runner.kickstart(arguments: ["kickstart", "-p", "gui/\(uid)/\(FreeformSnapshot.bundleID)"], timeout: timeout)
            if result.timedOut { return failure("Freeform snapshot timed out while starting helper.") }
            guard result.status == 0 else {
                return failure("Snapshot job could not start (launchctl exit \(result.status)). \(installationAdvice)")
            }
            while ProcessInfo.processInfo.systemUptime < deadline {
                if let status = lastStatus, status.created_at >= start {
                    let path = directory.appendingPathComponent("boards.db").path
                    if !status.ok {
                        return FreeformSnapshotResult(ok: false, snapshot_path: nil, created_at: status.created_at,
                                                      error: status.error ?? "Helper failed. Check Full Disk Access.")
                    }
                    guard FileManager.default.fileExists(atPath: path) else { return failure("Helper reported success without a snapshot.") }
                    return FreeformSnapshotResult(ok: true, snapshot_path: path, created_at: status.created_at, error: nil)
                }
                Thread.sleep(forTimeInterval: min(0.05, max(0, deadline - ProcessInfo.processInfo.systemUptime)))
            }
            return failure("Freeform snapshot timed out waiting for a fresh status.json. Check helper Full Disk Access and registration.")
        } catch { return failure("Cannot start snapshot helper: \(error.localizedDescription). \(installationAdvice)") }
    }

    private var installationAdvice: String {
        "Helper job not registered or unavailable. Run operate/scripts/freeform-snapshot-helper.sh, grant its app Full Disk Access, then run launchctl bootstrap gui/\(uid) \(plist.path)."
    }
}
