// ViewModels — @MainActor ObservableObject view models for the SymBrain screens.

#if os(macOS)
import Foundation
import AppKit
import SymairaCLIRunner
import SymairaToolKit
import SymairaUpdateCheck

// MARK: - AppUpdateStatus helpers

extension AppUpdateStatus {
    public var isInstalling: Bool {
        if case .installing = self { return true }
        return false
    }
}

// MARK: - DashboardViewModel

@MainActor
public final class DashboardViewModel: ObservableObject {
    @Published public var versionInfo: VersionInfo?
    @Published public var doctorReport: DoctorReport?
    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var isBinaryNotFound = false

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
        defer { isLoading = false }

        do {
            async let v = client.version()
            async let d = client.doctor()
            versionInfo = try await v
            doctorReport = try await d
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
            if let cliError = error as? CLIRunnerError,
               case .binaryNotFound = cliError {
                isBinaryNotFound = true
            }
        }
    }
}

// MARK: - ProfilesViewModel

@MainActor
public final class ProfilesViewModel: ObservableObject {
    @Published public var profiles: [ProfileSummary] = []
    @Published public var selectedProfile: ProfileDetail?
    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func loadProfiles() async {
        isLoading = true
        errorMessage = nil
        errorDetail = nil
        defer { isLoading = false }

        do {
            profiles = try await client.profileList()
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
        }
    }

    public func selectProfile(_ name: String) async {
        // Toggle: clicking the already-expanded profile collapses it.
        if selectedProfile?.name == name {
            selectedProfile = nil
            return
        }
        isLoading = true
        errorMessage = nil
        errorDetail = nil
        defer { isLoading = false }

        do {
            selectedProfile = try await client.profileShow(name: name)
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
        }
    }

    public func addProfile(name: String, from template: String?) async -> Bool {
        do {
            _ = try await client.profileAdd(name: name, from: template)
            await loadProfiles()
            return true
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
            return false
        }
    }

    public func removeProfile(name: String) async -> Bool {
        do {
            _ = try await client.profileRemove(name: name)
            selectedProfile = nil
            await loadProfiles()
            return true
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
            return false
        }
    }
}

// MARK: - HarnessesViewModel

@MainActor
public final class HarnessesViewModel: ObservableObject {
    @Published public var harnesses: [HarnessStatus] = []
    @Published public var profiles: [ProfileSummary] = []
    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var operationResult: String?

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        errorMessage = nil
        errorDetail = nil
        defer { isLoading = false }

        do {
            async let d = client.doctor()
            async let p = client.profileList()
            let report = try await d
            harnesses = report.harnesses
            profiles = try await p
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
        }
    }

    public func install(harness: String, profile: String, dryRun: Bool) async {
        do {
            operationResult = try await client.install(harness: harness, profile: profile, dryRun: dryRun)
            await refresh()
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
        }
    }

    public func uninstall(harness: String, dryRun: Bool) async {
        do {
            operationResult = try await client.uninstall(harness: harness, dryRun: dryRun)
            await refresh()
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
        }
    }
}

// MARK: - AuditViewModel

@MainActor
public final class AuditViewModel: ObservableObject {
    @Published public var entries: [AuditEntry] = []
    @Published public var profiles: [ProfileSummary] = []
    @Published public var selectedProfile: String?
    @Published public var isLoading = false

    private let client: SymBrainClient
    private let auditReader = AuditLogReader()

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        defer { isLoading = false }

        entries = auditReader.read(profile: selectedProfile)
    }

    public func loadProfiles() async {
        do {
            profiles = try await client.profileList()
        } catch {
            // Non-fatal: audit can work without profile list
        }
    }
}

// MARK: - SettingsViewModel

@MainActor
public final class SettingsViewModel: ObservableObject {
    @Published public var versionInfo: VersionInfo?
    @Published public var updateInfo: String?
    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var updateStatus: AppUpdateStatus = .unknown

    public let updateChecker: AppUpdateChecker
    @Published public var autoPrefs: UserDefaultsAutoUpdatePreferenceStore

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
        let prefs = UserDefaultsAutoUpdatePreferenceStore(keyPrefix: "com.symaira.brain")
        self.autoPrefs = prefs
        self.updateChecker = AppUpdateChecker(
            checker: UpdateChecker(owner: "danieljustus", repo: "symaira-brain"),
            store: UserDefaultsSkippedVersionStore(key: "com.symaira.brain.updateSkippedTag"),
            currentVersion: { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.0.0" },
            autoPrefs: prefs
        )
    }

    public func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            versionInfo = try await client.version()
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
        }
    }

    public func checkForUpdate() async {
        await updateChecker.checkForUpdate(force: true)
        refreshStatusFromChecker()
    }

    public func checkOnLaunchIfEnabled() async {
        await updateChecker.checkOnLaunchIfEnabled()
        refreshStatusFromChecker()
    }

    public func skipUpdate(_ release: ReleaseInfo) {
        updateChecker.skip(release)
        refreshStatusFromChecker()
    }

    public func installUpdate(_ release: ReleaseInfo) async {
        updateStatus = .installing(progress: 0)
        let applier = UpdateApplier(progress: { written, total in
            let pct = total > 0 ? Double(written) / Double(total) : 0
            Task { @MainActor in
                self.updateStatus = .installing(progress: pct)
            }
        })
        do {
            let installedURL = try await applier.applyBundle(release: release)
            // The installed .app is at installedURL. Trigger relaunch.
            updateStatus = .readyToRelaunch
            _ = installedURL
        } catch {
            updateStatus = .error(String(describing: error))
        }
    }

    nonisolated public func relaunchAfterUpdate() {
        let bundleURL = Bundle.main.bundleURL
        let config = NSWorkspace.OpenConfiguration()
        config.createsNewApplicationInstance = true
        NSWorkspace.shared.open(bundleURL, configuration: config) { _, _ in
            Task { @MainActor in
                NSApplication.shared.terminate(nil)
            }
        }
    }

    // MARK: - Helpers

    private func refreshStatusFromChecker() {
        updateStatus = updateChecker.status
        switch updateChecker.status {
        case .unknown:
            updateInfo = nil
        case .upToDate:
            updateInfo = "You are up to date"
        case .available(let release):
            updateInfo = "New version available: \(release.tagName)"
        case .skipped(let release):
            updateInfo = "Skipped version \(release.tagName)"
        case .installing(let progress):
            updateInfo = "Installing... \(Int(progress * 100))%"
        case .readyToRelaunch:
            updateInfo = "Update ready — relaunch to apply"
        case .error(let message):
            updateInfo = "Update check failed: \(message)"
        }
    }
}
// MARK: - SyncViewModel

@MainActor
public final class SyncViewModel: ObservableObject {
    @Published public var syncSummary: SyncSummary?
    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var dryRun = true
    @Published public var isBinaryNotFound = false
    /// True once the user has confirmed a live (writing) sync in this
    /// session. The first live sync always asks for confirmation; later
    /// ones run immediately (#148).
    @Published public private(set) var liveSyncConfirmed = false

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    /// The directory `symbrain sync` resolves relative targets against.
    /// The CLI defaults `--project` to its working directory, which the
    /// runner inherits from this app (#147).
    public var syncWorkingDirectory: String {
        FileManager.default.currentDirectoryPath
    }

    /// True when pressing Sync Now may run without a confirmation alert:
    /// dry-run previews are always safe, and a live sync only needs the
    /// one-time confirmation per session (#148).
    public var canSyncImmediately: Bool {
        dryRun || liveSyncConfirmed
    }

    /// Handles a Dry Run toggle change (#148). Entering dry-run mode
    /// refreshes the preview (safe — nothing is written). Leaving dry-run
    /// mode must NOT run a sync: it only clears the stale preview so
    /// results are shown as no longer current. Live syncs happen solely
    /// via Sync Now.
    public func dryRunChanged(to newValue: Bool) async {
        if newValue {
            await sync()
        } else {
            clearPreview()
        }
    }

    /// Records that the user confirmed a live sync for this session (#148).
    public func confirmLiveSync() {
        liveSyncConfirmed = true
    }

    /// Clears the stale preview without touching the filesystem (#148).
    public func clearPreview() {
        syncSummary = nil
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
    }

    /// Resolves a reported target path for display. The CLI reports paths
    /// relative to its project directory (e.g. "./CLAUDE.md"); they are
    /// resolved against `syncWorkingDirectory` so the UI shows the real
    /// absolute target (#147).
    public func displayPath(for target: SyncTargetStatus) -> String {
        if target.path.hasPrefix("/") {
            return target.path
        }
        return URL(fileURLWithPath: syncWorkingDirectory)
            .appendingPathComponent(target.path)
            .standardizedFileURL
            .path
    }

    public func sync() async {
        isLoading = true
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
        defer { isLoading = false }

        do {
            syncSummary = try await client.sync(dryRun: dryRun)
        } catch {
            let friendly = formatError(error)
            errorMessage = friendly.message
            errorDetail = friendly.detail
            if let cliError = error as? CLIRunnerError,
               case .binaryNotFound = cliError {
                isBinaryNotFound = true
            }
        }
    }
}
#endif
