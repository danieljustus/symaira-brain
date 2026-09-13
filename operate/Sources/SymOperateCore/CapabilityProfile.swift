import Foundation

/// A local, versioned description of an application's automation capabilities.
///
/// Profiles are descriptive hints for callers and never replace `ActionPolicy`:
/// the generic profile is deliberately empty, and unknown applications resolve
/// to it rather than receiving assumptions from another application.
public struct CapabilityProfile: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let appFamily: String
    public let capabilities: Set<String>

    public init(
        version: Int = CapabilityProfile.currentVersion,
        appFamily: String,
        capabilities: Set<String> = []
    ) {
        self.version = version
        self.appFamily = appFamily
        self.capabilities = capabilities
    }

    /// The conservative fallback used when no application-specific profile is known.
    public static let generic = CapabilityProfile(appFamily: "generic")

    private enum CodingKeys: String, CodingKey {
        case version
        case appFamily = "app_family"
        case capabilities
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let version = try container.decode(Int.self, forKey: .version)
        guard version == Self.currentVersion else {
            throw CapabilityProfileError.unsupportedVersion(version)
        }
        let appFamily = try container.decode(String.self, forKey: .appFamily)
        guard !appFamily.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw CapabilityProfileError.invalid("app_family must not be empty")
        }
        let capabilities = try container.decode(Set<String>.self, forKey: .capabilities)
        guard !capabilities.contains(where: { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else {
            throw CapabilityProfileError.invalid("capabilities must not contain empty names")
        }
        self.version = version
        self.appFamily = appFamily
        self.capabilities = capabilities
    }
}

/// A local profile document keyed by bundle identifier or application family.
///
/// A key containing a bundle identifier wins over a family match. The store
/// never performs network access; callers explicitly provide the local file.
public struct CapabilityProfileStore: Codable, Equatable, Sendable {
    public static let currentVersion = 1

    public let version: Int
    public let profiles: [String: CapabilityProfile]

    public init(
        version: Int = CapabilityProfileStore.currentVersion,
        profiles: [String: CapabilityProfile] = [:]
    ) {
        self.version = version
        self.profiles = profiles
    }

    public static let empty = CapabilityProfileStore()

    /// Resolves a profile by exact bundle ID, then family, then the safe generic fallback.
    public func profile(forBundleID bundleID: String?, appFamily: String? = nil) -> CapabilityProfile {
        if let bundleID, let profile = profiles[bundleID] {
            return profile
        }
        if let appFamily {
            if let profile = profiles[appFamily] {
                return profile
            }
            if let profile = profiles.values.first(where: { $0.appFamily == appFamily }) {
                return profile
            }
        }
        return .generic
    }

    /// Loads and validates one local JSON profile document.
    public static func load(from url: URL) throws -> CapabilityProfileStore {
        do {
            return try JSONDecoder().decode(Self.self, from: Data(contentsOf: url))
        } catch let error as CapabilityProfileError {
            throw error
        } catch {
            throw CapabilityProfileError.invalid(error.localizedDescription)
        }
    }

    private enum CodingKeys: String, CodingKey {
        case version
        case profiles
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let version = try container.decode(Int.self, forKey: .version)
        guard version == Self.currentVersion else {
            throw CapabilityProfileError.unsupportedVersion(version)
        }
        let profiles = try container.decode([String: CapabilityProfile].self, forKey: .profiles)
        guard !profiles.keys.contains(where: { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else {
            throw CapabilityProfileError.invalid("profile keys must not be empty")
        }
        self.version = version
        self.profiles = profiles
    }
}

/// Compatibility spelling for callers that refer to the collection as a profile store.
public typealias ProfileStore = CapabilityProfileStore

public enum CapabilityProfileError: LocalizedError, Equatable {
    case unsupportedVersion(Int)
    case invalid(String)

    public var errorDescription: String? {
        switch self {
        case .unsupportedVersion(let version):
            return "Unsupported capability profile version \(version)."
        case .invalid(let message):
            return "Invalid capability profile: \(message)."
        }
    }
}
