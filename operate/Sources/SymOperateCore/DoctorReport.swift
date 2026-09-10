import Foundation

public enum ExitCode: Int32 {
    case ok = 0
    case generalError = 1
    case permissionDenied = 2
    case notFound = 3
    case invalidArgument = 4
    case operationFailed = 5
    case staleReference = 6
    case unavailable = 7

    public var description: String {
        switch self {
        case .ok: return "OK"
        case .generalError: return "General error"
        case .permissionDenied: return "Permission denied"
        case .notFound: return "Not found"
        case .invalidArgument: return "Invalid argument"
        case .operationFailed: return "Operation failed"
        case .staleReference: return "Stale reference"
        case .unavailable: return "Unavailable"
        }
    }
}

public struct DoctorReport: Codable, Sendable {
    public let ok: Bool
    public let version: String
    public let permissions: PermissionSnapshot
    public let capabilities: [String: Bool]
    public let environment: EnvironmentReport
    public let recommendations: [String]
    /// The permission grant effective for this process after startup policy.
    public let effectiveGrant: [String]

    enum CodingKeys: String, CodingKey {
        case ok, version, permissions, capabilities, environment, recommendations
        case effectiveGrant = "effective_grant"
    }

    public init(
        ok: Bool,
        version: String,
        permissions: PermissionSnapshot,
        capabilities: [String: Bool],
        environment: EnvironmentReport,
        recommendations: [String],
        effectiveGrant: [String]
    ) {
        self.ok = ok
        self.version = version
        self.permissions = permissions
        self.capabilities = capabilities
        self.environment = environment
        self.recommendations = recommendations
        self.effectiveGrant = effectiveGrant
    }
}

public struct EnvironmentReport: Codable, Sendable {
    public let platform: String
    public let macOSVersion: String
    public let swiftVersion: String
    public let appsCount: Int
    public let displaysCount: Int

    public init(platform: String, macOSVersion: String, swiftVersion: String, appsCount: Int, displaysCount: Int) {
        self.platform = platform
        self.macOSVersion = macOSVersion
        self.swiftVersion = swiftVersion
        self.appsCount = appsCount
        self.displaysCount = displaysCount
    }
}

public struct ProbeResult: Codable, Sendable {
    public let ok: Bool
    public let message: String

    public init(ok: Bool, message: String) {
        self.ok = ok
        self.message = message
    }
}

public enum DoctorAdvice {
    /// Builds the doctor recommendation for a failed screenshot probe.
    ///
    /// A `.unavailable` capture failure means ScreenCaptureKit exposes no
    /// capturable display (headless/clamshell Mac, or a stale TCC identity
    /// after the binary was re-signed). That state needs actionable environment
    /// advice, not the raw capture error; every other failure keeps its
    /// localized message.
    public static func screenshotProbeRecommendation(for error: AutomationError) -> String {
        switch error {
        case let .unavailable(message):
            return "\(message) ScreenCaptureKit exposes no capturable display: connect a display to this Mac, or if the binary was recently re-signed, re-grant Screen Recording to the current binary identity (System Settings > Privacy & Security > Screen Recording)."
        default:
            return error.localizedDescription
        }
    }
}
