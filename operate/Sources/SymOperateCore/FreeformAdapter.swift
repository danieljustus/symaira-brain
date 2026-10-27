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
    /// Raw AXPosition write. Freeform moves the item by (point - current) × zoom, not to `point`.
    func setPosition(label: String, point: PointValue) throws
    /// Zoom, canvas origin and scroll state of the board window.
    func viewport() throws -> FreeformViewport
    /// Scroll bar values 0...1; x is horizontal, y vertical.
    func scroll(to position: PointValue) throws
    /// Screen frame of the visible object with this unique label.
    func frame(label: String) throws -> RectValue
    /// End handle of the single selected connection line, in screen coordinates.
    func selectedLineEnd(_ end: FreeformLineEnd) throws -> PointValue
    /// Raw AXPosition write on that handle; moves by (point - current) × zoom like items.
    func setSelectedLineEnd(_ end: FreeformLineEnd, to point: PointValue) throws
    /// AX description of the single selected line; it names both anchored objects once connected.
    func selectedLineDescription() throws -> String
}

public enum FreeformLineEnd: Sendable { case tail, head }

public struct FreeformConnectExistingRequest: Codable, Sendable {
    public let tailLabel: String
    public let headLabel: String

    enum CodingKeys: String, CodingKey { case tailLabel = "tail_label", headLabel = "head_label" }

    public init(tailLabel: String, headLabel: String) {
        self.tailLabel = tailLabel
        self.headLabel = headLabel
    }
}

public struct FreeformConnectExistingResult: Codable, Sendable {
    public let submitted: Bool
    /// Freeform's own AX description names both objects as the line's endpoints.
    public let anchored: Bool
    public let focusRestored: Bool
    public let pasteboardRestored: Bool
    public let scrolled: Bool
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case submitted, anchored, scrolled, error
        case focusRestored = "focus_restored", pasteboardRestored = "pasteboard_restored"
    }
}

/// AX only exposes canvas items that intersect the visible area (plus the selection).
public struct FreeformViewport: Equatable, Sendable {
    /// 1.0 is 100 %.
    public var zoom: Double
    /// Scroll bar values 0...1.
    public var scroll: PointValue
    /// Scroll positions per axis needed to cover the content (ceil(content / visible)).
    public var pages: PointValue

    public init(zoom: Double, scroll: PointValue = PointValue(x: 0, y: 0), pages: PointValue = PointValue(x: 1, y: 1)) {
        self.zoom = zoom
        self.scroll = scroll
        self.pages = pages
    }
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
    /// Optional assertion in percent. The adapter always measures the real zoom.
    public let zoom: Double?

    enum CodingKeys: String, CodingKey {
        case label, x, y, zoom
        case referenceLabel = "reference_label", referenceX = "reference_x", referenceY = "reference_y"
    }

    public init(label: String, referenceLabel: String, referenceX: Double, referenceY: Double, x: Double, y: Double,
                zoom: Double? = nil) {
        self.label = label
        self.referenceLabel = referenceLabel
        self.referenceX = referenceX
        self.referenceY = referenceY
        self.x = x
        self.y = y
        self.zoom = zoom
    }
}

public struct FreeformMoveResult: Codable, Sendable {
    public let confirmed: Bool
    public let screenPosition: PointValue
    public let boardPosition: PointValue
    public let corrected: Bool
    /// Measured zoom in percent.
    public let zoom: Double
    /// True when the board had to be scrolled to reach the reference or the target.
    public let scrolled: Bool

    enum CodingKeys: String, CodingKey {
        case confirmed, corrected, zoom, scrolled
        case screenPosition = "screen_position", boardPosition = "board_position"
    }
}

/// Board/screen transform calibrated from an object with a known board position at a measured zoom.
public struct FreeformBoardTransform: Sendable {
    public let board: PointValue
    public let screen: PointValue
    public let zoom: Double

    public init(board: PointValue, screen: PointValue, zoom: Double = 1) {
        self.board = board
        self.screen = screen
        self.zoom = zoom
    }

    public func screen(_ point: PointValue) -> PointValue {
        PointValue(x: screen.x + (point.x - board.x) * zoom, y: screen.y + (point.y - board.y) * zoom)
    }

    public func board(_ point: PointValue) -> PointValue {
        PointValue(x: board.x + (point.x - screen.x) / zoom, y: board.y + (point.y - screen.y) / zoom)
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
        let paste = try pasteTransaction(payload, boardTitle: boardTitle)
        return FreeformInsertResult(submitted: paste.submitted, objects: paste.submitted ? board.objects.map(\.id) : [],
                                    connectionCount: paste.submitted ? board.connections.count : 0,
                                    focusRestored: paste.focusRestored, pasteboardRestored: paste.pasteboardRestored,
                                    error: paste.error)
    }

    /// Connects two objects already on the board. Paste-time anchors only resolve inside the pasted
    /// payload (live-verified), so a free line is pasted and its two AX end handles are then set onto
    /// the object centers; Freeform anchors an end dropped onto an object, like a drag would.
    public func connectExisting(_ request: FreeformConnectExistingRequest, boardTitle: String,
                                policy: ActionPolicy) throws -> FreeformConnectExistingResult {
        try authorize(policy)
        guard !request.tailLabel.isEmpty, !request.headLabel.isEmpty, request.tailLabel != request.headLabel else {
            throw AutomationError.invalidArgument("Connecting requires two different nonempty labels.")
        }
        let payload = try FreeformContentLanguage.freeLine()
        lock.lock()
        defer { lock.unlock() }
        try Self.requireBoard(boardTitle, in: environment.doctor())
        let start = try environment.viewport()
        let zoom = start.zoom
        guard zoom.isFinite, zoom > 0 else { throw AutomationError.unavailable("Freeform reported an invalid zoom.") }
        var scrolled = false
        defer { if scrolled { try? environment.scroll(to: start.scroll) } }
        // Both objects must be reachable before anything is pasted.
        _ = try reveal(request.tailLabel, pages: start.pages, scrolled: &scrolled)
        _ = try reveal(request.headLabel, pages: start.pages, scrolled: &scrolled)
        if scrolled { try environment.scroll(to: start.scroll) }
        let paste = try pasteTransaction(payload, boardTitle: boardTitle)
        guard paste.submitted else {
            return FreeformConnectExistingResult(submitted: false, anchored: false, focusRestored: paste.focusRestored,
                                                 pasteboardRestored: paste.pasteboardRestored, scrolled: scrolled,
                                                 error: paste.error)
        }
        var failure = paste.error
        var anchored = false
        do {
            for (end, label) in [(FreeformLineEnd.tail, request.tailLabel), (.head, request.headLabel)] {
                _ = try reveal(label, pages: start.pages, scrolled: &scrolled)
                let frame = try environment.frame(label: label)
                let center = PointValue(x: frame.x + frame.width / 2, y: frame.y + frame.height / 2)
                let current = try environment.selectedLineEnd(end)
                try environment.setSelectedLineEnd(end, to: Self.axTarget(from: current, to: center, zoom: zoom))
                environment.pause(seconds: 0.3)
            }
            let description = try environment.selectedLineDescription()
            anchored = description.contains(request.tailLabel + ",") && description.contains(request.headLabel + ",")
            if !anchored { failure = [failure, "Line ends were set but Freeform did not report both anchors."]
                .compactMap { $0 }.joined(separator: "; ") }
        } catch {
            failure = [failure, error.localizedDescription].compactMap { $0 }.joined(separator: "; ")
        }
        return FreeformConnectExistingResult(submitted: true, anchored: anchored, focusRestored: paste.focusRestored,
                                             pasteboardRestored: paste.pasteboardRestored, scrolled: scrolled,
                                             error: failure)
    }

    private struct PasteOutcome {
        let submitted: Bool, focusRestored: Bool, pasteboardRestored: Bool, error: String?
    }

    /// Caller holds `lock`. Clipboard and focus are restored on every path.
    private func pasteTransaction(_ payload: [String: Data], boardTitle: String) throws -> PasteOutcome {
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
        return PasteOutcome(submitted: submitted, focusRestored: focusRestored, pasteboardRestored: pasteboardRestored,
                            error: failure)
    }

    public func move(_ request: FreeformMoveRequest, boardTitle: String, policy: ActionPolicy) throws -> FreeformMoveResult {
        try authorize(policy)
        guard !request.label.isEmpty, !request.referenceLabel.isEmpty,
              [request.x, request.y, request.referenceX, request.referenceY].allSatisfy(\.isFinite) else {
            throw AutomationError.invalidArgument("Move requires labels and finite board coordinates.")
        }
        lock.lock()
        defer { lock.unlock() }
        try Self.requireBoard(boardTitle, in: environment.doctor())
        let start = try environment.viewport()
        let zoom = start.zoom
        guard zoom.isFinite, zoom > 0 else { throw AutomationError.unavailable("Freeform reported an invalid zoom.") }
        if let asserted = request.zoom, abs(asserted / 100 - zoom) > 0.0005 {
            throw AutomationError.invalidArgument("Board zoom is \(zoom * 100)%, not the asserted \(asserted)%.")
        }
        var scrolled = false
        // The user's view is restored on every path; a move must not leave the board scrolled elsewhere.
        defer { if scrolled { try? environment.scroll(to: start.scroll) } }
        let referenceBoard = PointValue(x: request.referenceX, y: request.referenceY)
        let target = PointValue(x: request.x, y: request.y)
        // Reference first: once the target is selected it stays in the AX tree while scrolling.
        _ = try reveal(request.referenceLabel, pages: start.pages, scrolled: &scrolled)
        try reveal(request.label, pages: start.pages, scrolled: &scrolled)
        try environment.deselectIfAvailable()
        try environment.selectOnly(label: request.label)
        var transform = FreeformBoardTransform(board: referenceBoard, screen: try reveal(
            request.referenceLabel, pages: start.pages, scrolled: &scrolled), zoom: zoom)
        var expected = transform.screen(target)
        var actual = try environment.position(label: request.label)
        try environment.setPosition(label: request.label, point: Self.axTarget(from: actual, to: expected, zoom: zoom))
        environment.pause(seconds: 0.3)
        transform = FreeformBoardTransform(board: referenceBoard, screen: try reveal(
            request.referenceLabel, pages: start.pages, scrolled: &scrolled), zoom: zoom)
        expected = transform.screen(target)
        actual = try environment.position(label: request.label)
        let corrected = abs(actual.x - expected.x) >= 0.5 || abs(actual.y - expected.y) >= 0.5
        if corrected {
            // The residual is treated as a repeatable bias (as observed at 100%), so it is compensated twice:
            // at zoom 1 this is the previously live-verified write of expected + (expected - actual).
            let compensated = PointValue(x: 2 * expected.x - actual.x, y: 2 * expected.y - actual.y)
            try environment.setPosition(label: request.label, point: Self.axTarget(from: actual, to: compensated, zoom: zoom))
            environment.pause(seconds: 0.3)
            actual = try environment.position(label: request.label)
        }
        return FreeformMoveResult(confirmed: abs(actual.x - expected.x) < 0.5 && abs(actual.y - expected.y) < 0.5,
                                  screenPosition: actual, boardPosition: transform.board(actual), corrected: corrected,
                                  zoom: zoom * 100, scrolled: scrolled)
    }

    /// Live-measured on macOS 27.2: an AXPosition write moves the item by (written - current) × zoom.
    static func axTarget(from current: PointValue, to expected: PointValue, zoom: Double) -> PointValue {
        PointValue(x: current.x + (expected.x - current.x) / zoom, y: current.y + (expected.y - current.y) / zoom)
    }

    /// Returns the label's screen position, scrolling through the board grid when it is off-screen.
    @discardableResult
    private func reveal(_ label: String, pages: PointValue, scrolled: inout Bool) throws -> PointValue {
        if let position = try? environment.position(label: label) { return position }
        let columns = max(1, min(20, Int(pages.x.rounded(.up)))), rows = max(1, min(20, Int(pages.y.rounded(.up))))
        for row in 0...rows {
            for column in 0...columns {
                try environment.scroll(to: PointValue(x: Double(column) / Double(columns), y: Double(row) / Double(rows)))
                scrolled = true
                environment.pause(seconds: 0.15)
                if let position = try? environment.position(label: label) { return position }
            }
        }
        return try environment.position(label: label)
    }

    private func authorize(_ policy: ActionPolicy) throws {
        let scope = PolicyScope(application: FreeformDescriptor.bundleID)
        if policy.firstViolation(requiredPermission: Self.writePermissions, scope: scope) != nil {
            throw AutomationError.permissionDenied("Freeform writes require input, app_control and menu_action permissions.")
        }
    }
}
