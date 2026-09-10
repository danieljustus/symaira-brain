import Foundation

public enum SymOperateVersion {
    public static let current = "0.6.1"
}

/// Selects how input events are delivered to macOS.
///
/// `automatic` preserves the legacy system-wide event behavior. `background`
/// is fail-closed and requires a verified target process. `foreground` always
/// uses the legacy HID event path.
public enum DeliveryMode: String, Codable, Sendable, CaseIterable {
    case automatic
    case background
    case foreground
}

public struct PointValue: Codable, Sendable, Equatable {
    public let x: Double
    public let y: Double

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

public struct SizeValue: Codable, Sendable, Equatable {
    public let width: Double
    public let height: Double

    public init(width: Double, height: Double) {
        self.width = width
        self.height = height
    }
}

public struct RectValue: Codable, Sendable, Equatable {
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

    public var center: PointValue {
        PointValue(x: x + (width / 2.0), y: y + (height / 2.0))
    }
}

public struct DisplayInfo: Codable, Sendable {
    public let displayID: UInt32
    public let bounds: RectValue
    public let isMain: Bool

    public init(displayID: UInt32, bounds: RectValue, isMain: Bool) {
        self.displayID = displayID
        self.bounds = bounds
        self.isMain = isMain
    }
}

/// Identifies the process whose TCC grants the PermissionSnapshot booleans describe.
///
/// On macOS, TCC permissions (Accessibility, Screen Recording) are per-process.
/// The booleans in the enclosing `PermissionSnapshot` reflect the grants held by the
/// process captured here — typically the process running symoperate, which may be a
/// terminal shell, an IDE, or an MCP host (e.g. Claude Desktop, Cursor).
public struct PermissionSource: Codable, Sendable, Equatable {
    /// Process ID of the process whose grants are reported.
    public let pid: Int32
    /// Parent process ID (the process that launched symoperate).
    public let ppid: Int32
    /// Resolved absolute path to the executable that is running.
    public let executablePath: String
    /// Human-readable name of the launching/parent process, where obtainable.
    /// Examples: "Terminal", "zsh", "Cursor", "Claude Desktop", "Xcode".
    public let launchingProcessName: String?
    /// Explanatory note clarifying whose grants the booleans describe.
    public let note: String

    public init(pid: Int32, ppid: Int32, executablePath: String, launchingProcessName: String?, note: String) {
        self.pid = pid
        self.ppid = ppid
        self.executablePath = executablePath
        self.launchingProcessName = launchingProcessName
        self.note = note
    }
}

public struct PermissionSnapshot: Codable, Sendable {
    public let accessibilityGranted: Bool
    public let screenRecordingGranted: Bool
    /// Identifies the process whose TCC grants the booleans above describe.
    public let source: PermissionSource

    public init(accessibilityGranted: Bool, screenRecordingGranted: Bool, source: PermissionSource) {
        self.accessibilityGranted = accessibilityGranted
        self.screenRecordingGranted = screenRecordingGranted
        self.source = source
    }
}

public struct SnapshotTransform: Codable, Sendable {
    public let displayID: UInt32
    public let displayBounds: RectValue
    public let imageSize: SizeValue

    public init(displayID: UInt32, displayBounds: RectValue, imageSize: SizeValue) {
        self.displayID = displayID
        self.displayBounds = displayBounds
        self.imageSize = imageSize
    }

    public func imageToDisplay(point: PointValue) -> PointValue {
        let x = displayBounds.x + ((point.x / imageSize.width) * displayBounds.width)
        let y = displayBounds.y + ((point.y / imageSize.height) * displayBounds.height)
        return PointValue(x: x, y: y)
    }
}

public struct Snapshot: Codable, Sendable {
    public let id: String
    public let createdAt: String
    public let imageBase64PNG: String
    public let imageSize: SizeValue
    public let displayBounds: RectValue
    public let displayID: UInt32
    public let windowID: Int?
    public let windowOwnerPID: Int32?
    public let debugImagePath: String?
    public let transform: SnapshotTransform

    public init(
        id: String,
        createdAt: String,
        imageBase64PNG: String,
        imageSize: SizeValue,
        displayBounds: RectValue,
        displayID: UInt32,
        windowID: Int? = nil,
        windowOwnerPID: Int32? = nil,
        debugImagePath: String? = nil,
        transform: SnapshotTransform
    ) {
        self.id = id
        self.createdAt = createdAt
        self.imageBase64PNG = imageBase64PNG
        self.imageSize = imageSize
        self.displayBounds = displayBounds
        self.displayID = displayID
        self.windowID = windowID
        self.windowOwnerPID = windowOwnerPID
        self.debugImagePath = debugImagePath
        self.transform = transform
    }

    // MARK: - Codable (encodeIfPresent prevents local path leakage to MCP clients)

    private enum CodingKeys: String, CodingKey {
        case id, createdAt, imageBase64PNG, imageSize, displayBounds, displayID, windowID, windowOwnerPID, debugImagePath, transform
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(createdAt, forKey: .createdAt)
        try container.encode(imageBase64PNG, forKey: .imageBase64PNG)
        try container.encode(imageSize, forKey: .imageSize)
        try container.encode(displayBounds, forKey: .displayBounds)
        try container.encode(displayID, forKey: .displayID)
        try container.encodeIfPresent(windowID, forKey: .windowID)
        try container.encodeIfPresent(windowOwnerPID, forKey: .windowOwnerPID)
        try container.encodeIfPresent(debugImagePath, forKey: .debugImagePath)
        try container.encode(transform, forKey: .transform)
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        createdAt = try container.decode(String.self, forKey: .createdAt)
        imageBase64PNG = try container.decode(String.self, forKey: .imageBase64PNG)
        imageSize = try container.decode(SizeValue.self, forKey: .imageSize)
        displayBounds = try container.decode(RectValue.self, forKey: .displayBounds)
        displayID = try container.decode(UInt32.self, forKey: .displayID)
        windowID = try container.decodeIfPresent(Int.self, forKey: .windowID)
        windowOwnerPID = try container.decodeIfPresent(Int32.self, forKey: .windowOwnerPID)
        debugImagePath = try container.decodeIfPresent(String.self, forKey: .debugImagePath)
        transform = try container.decode(SnapshotTransform.self, forKey: .transform)
    }
}

public struct AppInfo: Codable, Sendable {
    public let localizedName: String
    public let bundleIdentifier: String?
    public let processIdentifier: Int32
    public let isActive: Bool

    public init(localizedName: String, bundleIdentifier: String?, processIdentifier: Int32, isActive: Bool) {
        self.localizedName = localizedName
        self.bundleIdentifier = bundleIdentifier
        self.processIdentifier = processIdentifier
        self.isActive = isActive
    }
}

public struct WindowInfo: Codable, Sendable {
    public let windowID: Int
    public let ownerName: String
    public let ownerPID: Int32
    public let title: String?
    public let bounds: RectValue
    public let layer: Int
    /// Display containing the window, when the platform can determine it.
    public let displayID: UInt32?
    /// macOS Space/workspace identifier, when the platform exposes it.
    public let spaceID: Int?

    public init(
        windowID: Int,
        ownerName: String,
        ownerPID: Int32,
        title: String?,
        bounds: RectValue,
        layer: Int,
        displayID: UInt32? = nil,
        spaceID: Int? = nil
    ) {
        self.windowID = windowID
        self.ownerName = ownerName
        self.ownerPID = ownerPID
        self.title = title
        self.bounds = bounds
        self.layer = layer
        self.displayID = displayID
        self.spaceID = spaceID
    }
}

public struct UINode: Codable, Sendable {
    public let id: String
    public let role: String?
    public let subrole: String?
    public let title: String?
    public let label: String?
    public let value: String?
    public let nodeDescription: String?
    public let frame: RectValue?
    public let actions: [String]
    public let enabled: Bool?
    public let children: [UINode]

    public init(
        id: String,
        role: String?,
        subrole: String?,
        title: String?,
        label: String?,
        value: String?,
        nodeDescription: String?,
        frame: RectValue?,
        actions: [String],
        enabled: Bool? = nil,
        children: [UINode]
    ) {
        self.id = id
        self.role = role
        self.subrole = subrole
        self.title = title
        self.label = label
        self.value = value
        self.nodeDescription = nodeDescription
        self.frame = frame
        self.actions = actions
        self.enabled = enabled
        self.children = children
    }
}

public struct UIQueryResult: Codable, Sendable {
    public let snapshot: Snapshot
    public let app: AppInfo?
    public let nodes: [UINode]

    public init(snapshot: Snapshot, app: AppInfo?, nodes: [UINode]) {
        self.snapshot = snapshot
        self.app = app
        self.nodes = nodes
    }
}

public struct UIQueryResultWithOCR: Codable, Sendable {
    public let snapshot: Snapshot
    public let app: AppInfo?
    public let nodes: [UINode]
    public let ocrResult: OCRResult?
    public let axTreeWeak: Bool

    public init(snapshot: Snapshot, app: AppInfo?, nodes: [UINode], ocrResult: OCRResult?, axTreeWeak: Bool) {
        self.snapshot = snapshot
        self.app = app
        self.nodes = nodes
        self.ocrResult = ocrResult
        self.axTreeWeak = axTreeWeak
    }
}

public enum AutomationError: LocalizedError {
    case permissionDenied(String)
    case unavailable(String)
    case unsupported(String)
    case unverifiable(String)
    case invalidArgument(String)
    case notFound(String)
    case operationFailed(String)
    case staleReference(String)
    case targetAmbiguous(String)
    case targetMismatch(String)
    case preconditionFailed(PredicateEvaluation)

    /// A stable, closed-vocabulary machine-readable code for this error case.
    /// Each case maps to one code; codes are additive-only and breaking changes
    /// to this set must be documented in SAFETY_AUDIT.md.
    public var code: String {
        switch self {
        case .permissionDenied:    return "destructive_control_refused"
        case .unavailable:         return "element_not_resolvable"
        case .unsupported:         return "unsupported_delivery_mode"
        case .unverifiable:        return "unverifiable_delivery"
        case .invalidArgument:     return "invalid_argument"
        case .notFound:            return "not_found"
        case .operationFailed:     return "operation_failed"
        case .staleReference:      return "stale_snapshot"
        case .targetAmbiguous:     return "ambiguous_target"
        case .targetMismatch:      return "target_mismatch"
        case .preconditionFailed:  return "precondition_failed"
        }
    }

    public var errorDescription: String? {
        switch self {
        case let .permissionDenied(message),
             let .unavailable(message),
             let .unsupported(message),
             let .unverifiable(message),
             let .invalidArgument(message),
             let .notFound(message),
             let .operationFailed(message),
             let .staleReference(message),
             let .targetAmbiguous(message),
             let .targetMismatch(message):
            return message
        case let .preconditionFailed(evaluation):
            return "Action precondition failed (\(evaluation.status.rawValue))."
                + (evaluation.reason.map { " \($0)" } ?? "")
        }
    }
}

public enum DateFormats {
    public static func iso8601String(from date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }
}

public struct HistoryEvent: Codable, Sendable {
    public let timestamp: String
    public let action: String
    public let targets: [String: String]
    public let success: Bool
    public let message: String
    /// Additive effect metadata. The legacy fields above remain unchanged.
    public let contractVersion: Int?
    public let effect: EffectState?
    public let verification: ActionVerification?
    public let executionPath: String?
    public let target: ActionTarget?
    /// Bounded, redacted evidence for reconstructing the action locally.
    public let diagnosticTrace: DiagnosticTraceRecord?

    public init(
        timestamp: String = DateFormats.iso8601String(from: Date()),
        action: String,
        targets: [String: String] = [:],
        success: Bool,
        message: String,
        contractVersion: Int? = nil,
        effect: EffectState? = nil,
        verification: ActionVerification? = nil,
        executionPath: String? = nil,
        target: ActionTarget? = nil,
        diagnosticTrace: DiagnosticTraceRecord? = nil
    ) {
        self.timestamp = timestamp
        self.action = action
        self.targets = targets
        self.success = success
        self.message = message
        self.contractVersion = contractVersion
        self.effect = effect
        self.verification = verification
        self.executionPath = executionPath
        self.target = target
        self.diagnosticTrace = diagnosticTrace
    }
}

