import AppKit
import Foundation
import SymCockpitHistory

/// A metadata-only event emitted by an activity source. It never contains UI
/// element values, screenshots, OCR, input, or clipboard data.
public struct ActivitySourceEvent: Equatable, Sendable {
    public enum Kind: String, Equatable, Sendable {
        case foregroundApp
        case foregroundWindow
    }

    public let timestamp: Date
    public let kind: Kind
    public let app: AppInfo
    public let windowTitle: String?
    public let windowBounds: ActivityWindowBounds?

    public init(
        timestamp: Date,
        kind: Kind,
        app: AppInfo,
        windowTitle: String? = nil,
        windowBounds: ActivityWindowBounds? = nil
    ) {
        self.timestamp = timestamp
        self.kind = kind
        self.app = app
        self.windowTitle = windowTitle
        self.windowBounds = windowBounds
    }

    public static func == (lhs: ActivitySourceEvent, rhs: ActivitySourceEvent) -> Bool {
        lhs.timestamp == rhs.timestamp
            && lhs.kind == rhs.kind
            && lhs.app.localizedName == rhs.app.localizedName
            && lhs.app.bundleIdentifier == rhs.app.bundleIdentifier
            && lhs.app.processIdentifier == rhs.app.processIdentifier
            && lhs.app.isActive == rhs.app.isActive
            && lhs.windowTitle == rhs.windowTitle
            && lhs.windowBounds == rhs.windowBounds
    }
}

public struct ActivityWindowBounds: Codable, Equatable, Sendable {
    public let x: Double
    public let y: Double
    public let width: Double
    public let height: Double

    public init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

/// Injectable boundary for event-driven foreground activity notifications.
public protocol ActivityEventSource: AnyObject {
    func start(handler: @escaping @Sendable (ActivitySourceEvent) -> Void)
    func stop()
}

/// The real event source. It listens to workspace activation notifications and
/// reads only foreground app/window metadata when macOS reports an activation.
/// It is deliberately not started by construction and has no polling timer.
public final class WorkspaceActivityEventSource: ActivityEventSource, @unchecked Sendable {
    private let workspace: NSWorkspace
    private let appService: any AppServiceProtocol
    private let lock = NSLock()
    private var token: NSObjectProtocol?
    private var handler: (@Sendable (ActivitySourceEvent) -> Void)?

    public init(workspace: NSWorkspace = .shared, appService: any AppServiceProtocol = AppService()) {
        self.workspace = workspace
        self.appService = appService
    }

    public func start(handler: @escaping @Sendable (ActivitySourceEvent) -> Void) {
        lock.lock()
        guard self.handler == nil else {
            lock.unlock()
            return
        }
        self.handler = handler
        lock.unlock()

        token = workspace.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification,
            object: nil,
            queue: .main
        ) { [weak self] notification in
            self?.activated(notification)
        }

        activated(nil)
    }

    public func stop() {
        lock.lock()
        let currentToken = token
        token = nil
        handler = nil
        lock.unlock()
        if let currentToken {
            workspace.notificationCenter.removeObserver(currentToken)
        }
    }

    private func activated(_ notification: Notification?) {
        let app = (notification?.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication)
            .flatMap { runningApp in
                AppInfo(
                    localizedName: runningApp.localizedName ?? "Unknown",
                    bundleIdentifier: runningApp.bundleIdentifier,
                    processIdentifier: runningApp.processIdentifier,
                    isActive: true
                )
            } ?? appService.frontmostApp()
        guard let app else { return }

        let timestamp = Date()
        emit(ActivitySourceEvent(timestamp: timestamp, kind: .foregroundApp, app: app))
        guard let window = appService.frontmostWindow(ownerPID: app.processIdentifier, title: nil) else { return }
        emit(ActivitySourceEvent(
            timestamp: timestamp,
            kind: .foregroundWindow,
            app: app,
            windowTitle: window.title,
            windowBounds: ActivityWindowBounds(
                x: window.bounds.x,
                y: window.bounds.y,
                width: window.bounds.width,
                height: window.bounds.height
            )
        ))
    }

    private func emit(_ event: ActivitySourceEvent) {
        lock.lock()
        let currentHandler = handler
        lock.unlock()
        currentHandler?(event)
    }
}

/// Redaction at the write boundary. ActionPolicy remains the source of the
/// deny-list decision; this adds only the unavoidable secret-shaped value and
/// URL sanitisation checks required for activity metadata.
public enum ActivityRedactor {
    public static let policyVersion = "action-policy-v1"

    private static let secretPattern = try! NSRegularExpression(
        pattern: #"(?i)\b(password|passcode|secret|token|api[_-]?key)\b\s*[:=]"#
    )
    public static func redact(_ event: ActivitySourceEvent, policy: ActionPolicy) -> ActivitySourceEvent {
        ActivitySourceEvent(
            timestamp: event.timestamp,
            kind: event.kind,
            app: event.app,
            windowTitle: event.windowTitle.flatMap { redactWindowTitle($0, policy: policy) },
            windowBounds: event.windowBounds
        )
    }

    public static func redactWindowTitle(_ title: String, policy: ActionPolicy) -> String? {
        guard !title.isEmpty else { return nil }
        if policy.firstViolation(title: title) != nil { return nil }

        var redactedTitle = title
        if var components = URLComponents(string: title), components.scheme != nil {
            components.query = nil
            components.fragment = nil
            redactedTitle = components.string ?? title
        }

        let range = NSRange(redactedTitle.startIndex..., in: redactedTitle)
        if secretPattern.firstMatch(in: redactedTitle, range: range) != nil
            || SecretRedactor.redact(redactedTitle) != redactedTitle {
            return nil
        }
        return redactedTitle
    }
}

public final class ActivitySegmentStore: @unchecked Sendable {
    public static let segmentDuration: TimeInterval = 10 * 60
    public static let defaultRetention: TimeInterval = 48 * 60 * 60
    public static let defaultMaxSegments = 288

    public let rootURL: URL
    public let retention: TimeInterval
    public let maxSegments: Int
    private let maxEventsPerSegment: Int
    private let lock = NSLock()

    public init(
        rootURL: URL,
        retention: TimeInterval = ActivitySegmentStore.defaultRetention,
        maxSegments: Int = ActivitySegmentStore.defaultMaxSegments,
        maxEventsPerSegment: Int = 1000
    ) {
        self.rootURL = rootURL
        self.retention = max(0, retention)
        self.maxSegments = max(1, maxSegments)
        self.maxEventsPerSegment = max(1, maxEventsPerSegment)
        secureDirectory(rootURL)
    }

    public func append(_ event: ActivitySourceEvent, policy: ActionPolicy, now: Date? = nil) throws {
        lock.lock()
        defer { lock.unlock() }

        let retentionNow = now ?? Date()
        _ = try purgeExpiredLocked(at: retentionNow)
        let redacted = ActivityRedactor.redact(event, policy: policy)
        let segmentStart = Self.segmentStart(for: event.timestamp)
        let segmentURL = url(for: segmentStart)
        secureDirectory(segmentURL)

        let historyStore = CanonicalHistoryStore(
            fileURL: segmentURL.appendingPathComponent("events.jsonl"),
            maxEvents: maxEventsPerSegment
        )
        let payload = payload(for: redacted)
        try historyStore.append(CanonicalHistoryEvent(
            source: "activity",
            timestamp: DateFormats.iso8601String(from: redacted.timestamp),
            action: redacted.kind.rawValue,
            payload: payload
        ))

        var metadata = readMetadata(at: segmentURL) ?? ActivitySegmentMetadata(
            segmentStart: DateFormats.iso8601String(from: segmentStart),
            segmentEnd: DateFormats.iso8601String(from: segmentStart.addingTimeInterval(Self.segmentDuration)),
            eventCount: 0,
            bundleIDs: [],
            windowBounds: [],
            schemaVersion: 1,
            redactionPolicyVersion: ActivityRedactor.policyVersion
        )
        metadata.eventCount = min(metadata.eventCount + 1, maxEventsPerSegment)
        if let bundleID = redacted.app.bundleIdentifier, !metadata.bundleIDs.contains(bundleID) {
            metadata.bundleIDs.append(bundleID)
        }
        if let bounds = redacted.windowBounds, !metadata.windowBounds.contains(bounds) {
            metadata.windowBounds.append(bounds)
        }
        metadata.bundleIDs.sort()
        metadata.windowBounds.sort { ($0.x, $0.y, $0.width, $0.height) < ($1.x, $1.y, $1.width, $1.height) }
        try writeMetadata(metadata, to: segmentURL)
        try enforceMaximumSegmentsLocked()
    }

    public func segmentURL(for date: Date) -> URL {
        url(for: Self.segmentStart(for: date))
    }

    public func events(inSegmentFor date: Date) throws -> [CanonicalHistoryEvent] {
        let store = CanonicalHistoryStore(
            fileURL: segmentURL(for: date).appendingPathComponent("events.jsonl"),
            maxEvents: maxEventsPerSegment
        )
        return try store.read(limit: nil)
    }

    @discardableResult
    public func purgeExpired(at date: Date = Date()) throws -> Int {
        lock.lock()
        defer { lock.unlock() }
        return try purgeExpiredLocked(at: date)
    }

    public func deleteSegment(for date: Date) throws {
        lock.lock()
        defer { lock.unlock() }
        let target = segmentURL(for: date)
        if FileManager.default.fileExists(atPath: target.path) {
            try FileManager.default.removeItem(at: target)
        }
    }

    public func deleteAll() throws {
        lock.lock()
        defer { lock.unlock() }
        guard FileManager.default.fileExists(atPath: rootURL.path) else { return }
        for url in try FileManager.default.contentsOfDirectory(at: rootURL, includingPropertiesForKeys: nil) {
            try FileManager.default.removeItem(at: url)
        }
        secureDirectory(rootURL)
    }

    private func payload(for event: ActivitySourceEvent) -> [String: HistoryJSONValue] {
        var payload: [String: HistoryJSONValue] = [
            "app_name": .string(event.app.localizedName),
            "process_id": .integer(Int64(event.app.processIdentifier)),
        ]
        if let bundleID = event.app.bundleIdentifier { payload["bundle_id"] = .string(bundleID) }
        if let title = event.windowTitle { payload["window_title"] = .string(title) }
        if let bounds = event.windowBounds {
            payload["window_bounds"] = .object([
                "height": .number(bounds.height),
                "width": .number(bounds.width),
                "x": .number(bounds.x),
                "y": .number(bounds.y),
            ])
        }
        return payload
    }

    private func purgeExpiredLocked(at date: Date) throws -> Int {
        let cutoff = date.addingTimeInterval(-retention)
        var deleted = 0
        for segment in try segmentDirectories() {
            guard let start = Self.dateFormatter.date(from: segment.lastPathComponent) else { continue }
            if start < cutoff {
                try FileManager.default.removeItem(at: segment)
                deleted += 1
            }
        }
        return deleted
    }

    private func enforceMaximumSegmentsLocked() throws {
        let segments = try segmentDirectories().sorted { $0.lastPathComponent < $1.lastPathComponent }
        guard segments.count > maxSegments else { return }
        for segment in segments.prefix(segments.count - maxSegments) {
            try FileManager.default.removeItem(at: segment)
        }
    }

    private func segmentDirectories() throws -> [URL] {
        try FileManager.default.contentsOfDirectory(at: rootURL, includingPropertiesForKeys: [.isDirectoryKey])
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
    }

    private func url(for start: Date) -> URL {
        rootURL.appendingPathComponent(Self.dateFormatter.string(from: start), isDirectory: true)
    }

    private func readMetadata(at segment: URL) -> ActivitySegmentMetadata? {
        guard let data = try? Data(contentsOf: segment.appendingPathComponent("metadata.json")) else { return nil }
        return try? JSONDecoder().decode(ActivitySegmentMetadata.self, from: data)
    }

    private func writeMetadata(_ metadata: ActivitySegmentMetadata, to segment: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(metadata)
        try data.write(to: segment.appendingPathComponent("metadata.json"), options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: segment.appendingPathComponent("metadata.json").path)
    }

    private func secureDirectory(_ url: URL) {
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
    }

    private static func segmentStart(for date: Date) -> Date {
        let seconds = floor(date.timeIntervalSince1970 / segmentDuration) * segmentDuration
        return Date(timeIntervalSince1970: seconds)
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd'T'HH-mm-ss'Z'"
        return formatter
    }()
}

public struct ActivitySegmentMetadata: Codable, Equatable, Sendable {
    public var segmentStart: String
    public var segmentEnd: String
    public var eventCount: Int
    public var bundleIDs: [String]
    public var windowBounds: [ActivityWindowBounds]
    public let schemaVersion: Int
    public let redactionPolicyVersion: String

    public init(
        segmentStart: String,
        segmentEnd: String,
        eventCount: Int,
        bundleIDs: [String],
        windowBounds: [ActivityWindowBounds],
        schemaVersion: Int,
        redactionPolicyVersion: String
    ) {
        self.segmentStart = segmentStart
        self.segmentEnd = segmentEnd
        self.eventCount = eventCount
        self.bundleIDs = bundleIDs
        self.windowBounds = windowBounds
        self.schemaVersion = schemaVersion
        self.redactionPolicyVersion = redactionPolicyVersion
    }

    private enum CodingKeys: String, CodingKey {
        case segmentStart = "segment_start"
        case segmentEnd = "segment_end"
        case eventCount = "event_count"
        case bundleIDs = "bundle_ids"
        case windowBounds = "window_bounds"
        case schemaVersion = "schema_version"
        case redactionPolicyVersion = "redaction_policy_version"
    }
}

/// Opt-in coordinator. Construction is inert; only `start()` begins observing.
public final class ActivityObserver: @unchecked Sendable {
    private let source: any ActivityEventSource
    private let store: ActivitySegmentStore
    private let policy: ActionPolicy
    private let lock = NSLock()
    private var running = false

    public init(source: any ActivityEventSource, store: ActivitySegmentStore, policy: ActionPolicy = ActionPolicy()) {
        self.source = source
        self.store = store
        self.policy = policy
    }

    public var isRunning: Bool {
        lock.lock()
        defer { lock.unlock() }
        return running
    }

    public func start() {
        lock.lock()
        guard !running else {
            lock.unlock()
            return
        }
        running = true
        lock.unlock()
        source.start { [weak self] event in
            guard let self else { return }
            try? self.store.append(event, policy: self.policy)
        }
    }

    public func stop() {
        lock.lock()
        guard running else {
            lock.unlock()
            return
        }
        running = false
        lock.unlock()
        source.stop()
    }
}
