import Foundation
import SymairaUpdateCheck

/// High-level update checker for symoperate that wraps `SymairaUpdateCheck.UpdateChecker`
/// with a UserDefaults-backed skip-version store and backward-compatible `UpdateInfo` type.
public struct UpdateChecker: Sendable {
    private let underlying: SymairaUpdateCheck.UpdateChecker
    private let store: UserDefaultsSkippedVersionStore
    private let currentVersion: String

    private static let defaultsKey = "com.symaira.operate.updateSkippedTag"

    public init(
        currentVersion: String = SymOperateVersion.current,
        repoOwner: String = "danieljustus",
        repoName: String = "symaira-cockpit",
        client: UpdateHTTPClient? = nil,
        cacheDirectory: URL? = nil,
        userDefaults: UserDefaults = .standard
    ) {
        self.currentVersion = currentVersion
        self.underlying = SymairaUpdateCheck.UpdateChecker(
            owner: repoOwner,
            repo: repoName,
            client: client ?? URLSession.shared,
            cacheTTL: 24 * 60 * 60,
            cacheDirectory: cacheDirectory
        )
        self.store = UserDefaultsSkippedVersionStore(key: Self.defaultsKey, defaults: userDefaults)
    }

    /// Keep the existing initializer for backwards compatibility with callers that pass a custom session.
    /// Migrates to the appkit-backed implementation.
    @available(*, deprecated, message: "Use init(currentVersion:repoOwner:repoName:client:) instead")
    public init(currentVersion: String, repoOwner: String, repoName: String, session: URLSession) {
        self.init(currentVersion: currentVersion, repoOwner: repoOwner, repoName: repoName, client: session)
    }

    public struct UpdateInfo: Codable, Sendable {
        public let updateAvailable: Bool
        public let latestVersion: String?
        public let currentVersion: String
        public let releaseURL: String?
        public let error: String?

        public init(updateAvailable: Bool, latestVersion: String?, currentVersion: String, releaseURL: String?, error: String? = nil) {
            self.updateAvailable = updateAvailable
            self.latestVersion = latestVersion
            self.currentVersion = currentVersion
            self.releaseURL = releaseURL
            self.error = error
        }
    }

    /// Retained for backward compatibility with existing tests.
    public static let timeoutInterval: TimeInterval = 8

    /// Check for a newer release.
    ///
    /// This is a non-blocking async call. Results are cached on disk with a 24-hour TTL
    /// (managed by `SymairaUpdateCheck.UpdateChecker`), so frequent calls are cheap.
    /// If the latest release matches the user's skipped version (via `skipVersion()`),
    /// no update will be reported unless `force` is true.
    public func checkForUpdate(force: Bool = false) async -> UpdateInfo {
        // Dev builds are never checked.
        if SymairaUpdateCheck.StableVersion(currentVersion) == nil {
            return UpdateInfo(
                updateAvailable: false,
                latestVersion: nil,
                currentVersion: currentVersion,
                releaseURL: nil
            )
        }

        do {
            if let release = try await underlying.check(currentVersion: currentVersion, force: force) {
                // If the user has skipped this exact tag, suppress the update prompt.
                if !force, store.skippedTag() == release.tagName {
                    return UpdateInfo(
                        updateAvailable: false,
                        latestVersion: stripVPrefix(release.tagName),
                        currentVersion: currentVersion,
                        releaseURL: release.htmlURL
                    )
                }
                return UpdateInfo(
                    updateAvailable: true,
                    latestVersion: stripVPrefix(release.tagName),
                    currentVersion: currentVersion,
                    releaseURL: release.htmlURL
                )
            } else {
                return UpdateInfo(
                    updateAvailable: false,
                    latestVersion: nil,
                    currentVersion: currentVersion,
                    releaseURL: nil
                )
            }
        } catch {
            return UpdateInfo(
                updateAvailable: false,
                latestVersion: nil,
                currentVersion: currentVersion,
                releaseURL: nil,
                error: error.localizedDescription
            )
        }
    }

    /// Mark the given version as skipped so it will not be offered again.
    ///
    /// Accepts versions with or without a leading "v" prefix.
    /// The skipped tag persists in UserDefaults between app launches.
    public func skipVersion(_ version: String) {
        let tag = version.hasPrefix("v") ? version : "v" + version
        store.setSkippedTag(tag)
    }

    /// The currently skipped version (without "v" prefix), if any.
    public var skippedVersion: String? {
        guard let tag = store.skippedTag() else { return nil }
        return stripVPrefix(tag)
    }

    /// Clear the skipped version so the next check will offer it again.
    public func clearSkippedVersion() {
        store.setSkippedTag(nil)
    }

    /// Clear the process-lifetime cache (delegates to disk-cache removal).
    public static func clearCache() {
        // The appkit cache is disk-based with a TTL; there's no process-lifetime
        // cache to clear. This is a no-op for API compatibility.
    }

    // MARK: - Helpers

    private func stripVPrefix(_ tag: String) -> String {
        tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
    }
}
