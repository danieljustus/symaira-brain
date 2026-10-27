import Foundation

// swiftlint:disable identifier_name
// Board x/y intentionally match the existing PointValue API and the IR wire schema.

public enum FreeformCapability: String, Codable, CaseIterable, Hashable, Sendable {
    case insert, move, connect
    case readSnapshot = "read_snapshot"
}

public struct FreeformDescriptor: Codable, Equatable, Sendable {
    public static let currentVersion = 1
    public static let bundleID = "com.apple.freeform"
    public let version: Int
    public let bundleIdentifier: String
    public let appFamily: String
    public let capabilities: Set<FreeformCapability>

    public init() {
        version = Self.currentVersion
        bundleIdentifier = Self.bundleID
        appFamily = "freeform"
        capabilities = Set(FreeformCapability.allCases)
    }

    public var capabilityProfile: CapabilityProfile {
        CapabilityProfile(appFamily: appFamily, capabilities: Set(capabilities.map(\.rawValue)))
    }
}

/// Every pasteboard item, including every materialized type. No string-only backup.
public typealias FreeformPasteboardSnapshot = [[String: Data]]

public protocol FreeformPasteboardProtocol {
    func snapshot() throws -> FreeformPasteboardSnapshot
    /// Replaces the pasteboard with one item carrying every given type.
    func write(_ types: [String: Data]) throws
    /// Returns true only after readback equals the saved items and types.
    func restore(_ snapshot: FreeformPasteboardSnapshot) -> Bool
}

public struct FreeformDoctor: Codable, Sendable {
    public let running: Bool
    public let accessibility: Bool
    public let windowCount: Int
    public let processID: Int32?
    /// AX titles of Freeform's windows; the board title a write tool must name.
    public let windowTitles: [String]
    public var helperInstalled = false
    public var helperLastStatus: FreeformHelperStatus?

    enum CodingKeys: String, CodingKey {
        case helperInstalled = "helper_installed", helperLastStatus = "helper_last_status"
        case running, accessibility
        case windowCount = "window_count", processID = "process_id", windowTitles = "window_titles"
    }

    public init(running: Bool, accessibility: Bool, windowCount: Int, processID: Int32?, windowTitles: [String] = []) {
        self.running = running
        self.accessibility = accessibility
        self.windowCount = windowCount
        self.processID = processID
        self.windowTitles = windowTitles
    }
}

/// System-facing seam. Deliberately has no keystroke/event-injection operation.
public protocol FreeformEnvironmentProtocol {
    func doctor() -> FreeformDoctor
    func frontmostPID() -> Int32?
    func bringToFront(pid: Int32) throws
    func pasteEnabled() throws -> Bool
    func deselectIfAvailable() throws
    func pressPaste() throws
    func pause(seconds: Double)
    func position(label: String) throws -> PointValue
    func selectOnly(label: String) throws
    func setPosition(label: String, point: PointValue) throws
}

public struct FreeformInsertResult: Codable, Sendable {
    public let submitted: Bool
    public let objects: [UUID]
    public let connectionCount: Int
    public let focusRestored: Bool
    public let pasteboardRestored: Bool
    public let error: String?
    /// AXPress confirms submission, not a verified Freeform document mutation.
    public let effect = "unverifiable"

    enum CodingKeys: String, CodingKey {
        case submitted, objects, error, effect
        case connectionCount = "connection_count", focusRestored = "focus_restored"
        case pasteboardRestored = "pasteboard_restored"
    }
}

public struct FreeformMoveRequest: Codable, Sendable {
    public let label: String
    public let referenceLabel: String
    public let referenceX: Double
    public let referenceY: Double
    public let x: Double
    public let y: Double

    enum CodingKeys: String, CodingKey {
        case label, x, y
        case referenceLabel = "reference_label", referenceX = "reference_x", referenceY = "reference_y"
    }

    public init(label: String, referenceLabel: String, referenceX: Double, referenceY: Double, x: Double, y: Double) {
        self.label = label
        self.referenceLabel = referenceLabel
        self.referenceX = referenceX
        self.referenceY = referenceY
        self.x = x
        self.y = y
    }
}

public struct FreeformMoveResult: Codable, Sendable {
    public let confirmed: Bool
    public let screenPosition: PointValue
    public let boardPosition: PointValue
    public let corrected: Bool

    enum CodingKeys: String, CodingKey {
        case confirmed, corrected
        case screenPosition = "screen_position", boardPosition = "board_position"
    }
}

/// Explicit 100% zoom transform calibrated from an object with a known board position.
public struct FreeformBoardTransform: Sendable {
    public let offset: PointValue

    public init(board: PointValue, screen: PointValue) {
        offset = PointValue(x: screen.x - board.x, y: screen.y - board.y)
    }

    public func screen(_ point: PointValue) -> PointValue {
        PointValue(x: point.x + offset.x, y: point.y + offset.y)
    }

    public func board(_ point: PointValue) -> PointValue {
        PointValue(x: point.x - offset.x, y: point.y - offset.y)
    }
}

/// Shared synchronous transaction boundary. Serializes clipboard/focus and canvas operations.
/// Policy is supplied per call, so set_policy narrowing cannot leave a cached wider grant.
public final class FreeformAdapter: AutomationAdapter, @unchecked Sendable {
    public let descriptor = FreeformDescriptor()
    public let metadata = AutomationAdapterMetadata(
        identifier: "freeform", kind: .accessibility, appFamily: "freeform",
        bundleIdentifier: FreeformDescriptor.bundleID, capabilities: [.execute, .observe],
        requiredPermissions: [.accessibility], timeout: 5
    )
    public static let writePermissions: PermissionFlags = [.input, .appControl, .menuAction]
    private let environment: any FreeformEnvironmentProtocol
    private let pasteboard: any FreeformPasteboardProtocol
    private let lock = NSLock()
    public let snapshotService: any FreeformSnapshotServiceProtocol

    public init(environment: any FreeformEnvironmentProtocol = NativeFreeformEnvironment(),
                pasteboard: any FreeformPasteboardProtocol = NativeFreeformPasteboard(),
                snapshotService: any FreeformSnapshotServiceProtocol = FreeformSnapshotService()) {
        self.environment = environment
        self.pasteboard = pasteboard
        self.snapshotService = snapshotService
    }

    public func doctor() -> FreeformDoctor {
        lock.lock()
        defer { lock.unlock() }
        var result = environment.doctor()
        result.helperInstalled = snapshotService.helperInstalled
        result.helperLastStatus = snapshotService.lastStatus
        return result
    }

    public func plan(policy: ActionPolicy, name: String) -> DryRunPlan {
        let permissions = policy.effectivePermissions(for: PolicyScope(application: FreeformDescriptor.bundleID))
        return DryRunPlanner(policy: ActionPolicy(grantedPermissions: permissions))
            .plan([DryRunAction(name: name, permission: Self.writePermissions)])
    }

    /// Writes act on whatever board is open, so the caller must name it. Exactly one Freeform window
    /// with exactly this title is required; anything else fails closed before any side effect.
    /// (A live test once pasted into a user's real board because this check was missing.)
    static func requireBoard(_ title: String, in status: FreeformDoctor) throws {
        guard !title.isEmpty, status.windowTitles == [title] else {
            throw AutomationError.invalidArgument(
                "Expected exactly one open Freeform board titled \"\(title)\"; open windows: \(status.windowTitles).")
        }
    }

    public func insert(_ board: FreeformBoardIR, boardTitle: String, policy: ActionPolicy) throws -> FreeformInsertResult {
        try authorize(policy)
        let payload = try FreeformContentLanguage.pasteboardItem(board)
        lock.lock()
        defer { lock.unlock() }
        let status = environment.doctor()
        guard status.accessibility, let freeformPID = status.processID, status.windowCount > 0 else {
            throw AutomationError.unavailable("Freeform needs Accessibility permission and an open board window.")
        }
        try Self.requireBoard(boardTitle, in: status)
        guard let previousPID = environment.frontmostPID() else {
            throw AutomationError.unavailable("Cannot identify the previous frontmost process.")
        }
        let backup = try pasteboard.snapshot()
        var submitted = false
        var failure: String?
        do {
            try pasteboard.write(payload)
            try environment.bringToFront(pid: freeformPID)
            var ready = false
            for _ in 0..<40 {
                if try environment.pasteEnabled() { ready = true; break }
                environment.pause(seconds: 0.05)
            }
            guard ready else { throw AutomationError.unavailable("Freeform Paste did not become enabled within 2 seconds.") }
            try environment.deselectIfAvailable()
            try environment.pressPaste()
            submitted = true
            environment.pause(seconds: 0.3)
        } catch {
            failure = error.localizedDescription
        }
        // Cleanup is unconditional and independent: a focus error must not skip clipboard restoration.
        // Freeform can re-activate itself shortly after a paste lands (observed live), so the
        // restore is re-asserted until the previous app is still frontmost after a settle delay.
        var focusRestored = false
        var focusError: String?
        for _ in 0..<3 {
            do { try environment.bringToFront(pid: previousPID); focusError = nil }
            catch { focusError = error.localizedDescription }
            environment.pause(seconds: 0.25)
            if environment.frontmostPID() == previousPID { focusRestored = true; break }
        }
        if !focusRestored {
            failure = [failure, "Focus restoration: " + (focusError ?? "previous app did not stay frontmost.")]
                .compactMap { $0 }.joined(separator: "; ")
        }
        let pasteboardRestored = pasteboard.restore(backup)
        return FreeformInsertResult(submitted: submitted, objects: submitted ? board.objects.map(\.id) : [],
                                    connectionCount: submitted ? board.connections.count : 0,
                                    focusRestored: focusRestored, pasteboardRestored: pasteboardRestored, error: failure)
    }

    public func move(_ request: FreeformMoveRequest, boardTitle: String, policy: ActionPolicy) throws -> FreeformMoveResult {
        try authorize(policy)
        guard !request.label.isEmpty, !request.referenceLabel.isEmpty,
              [request.x, request.y, request.referenceX, request.referenceY].allSatisfy(\.isFinite) else {
            throw AutomationError.invalidArgument("Move requires labels and finite board coordinates at 100% zoom.")
        }
        lock.lock()
        defer { lock.unlock() }
        try Self.requireBoard(boardTitle, in: environment.doctor())
        let reference = try environment.position(label: request.referenceLabel)
        let transform = FreeformBoardTransform(board: PointValue(x: request.referenceX, y: request.referenceY), screen: reference)
        let expected = transform.screen(PointValue(x: request.x, y: request.y))
        try environment.deselectIfAvailable()
        try environment.selectOnly(label: request.label)
        try environment.setPosition(label: request.label, point: expected)
        environment.pause(seconds: 0.3)
        var actual = try environment.position(label: request.label)
        let corrected = abs(actual.x - expected.x) >= 0.5 || abs(actual.y - expected.y) >= 0.5
        if corrected {
            try environment.setPosition(label: request.label, point: PointValue(
                x: expected.x + expected.x - actual.x, y: expected.y + expected.y - actual.y))
            environment.pause(seconds: 0.3)
            actual = try environment.position(label: request.label)
        }
        return FreeformMoveResult(confirmed: abs(actual.x - expected.x) < 0.5 && abs(actual.y - expected.y) < 0.5,
                                  screenPosition: actual, boardPosition: transform.board(actual), corrected: corrected)
    }

    private func authorize(_ policy: ActionPolicy) throws {
        let scope = PolicyScope(application: FreeformDescriptor.bundleID)
        if policy.firstViolation(requiredPermission: Self.writePermissions, scope: scope) != nil {
            throw AutomationError.permissionDenied("Freeform writes require input, app_control and menu_action permissions.")
        }
    }
}
