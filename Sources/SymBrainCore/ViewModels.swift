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

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func loadProfiles() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            profiles = try await client.profileList()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func selectProfile(_ name: String) async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            selectedProfile = try await client.profileShow(name: name)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func addProfile(name: String, from template: String?) async -> Bool {
        do {
            _ = try await client.profileAdd(name: name, from: template)
            await loadProfiles()
            return true
        } catch {
            errorMessage = error.localizedDescription
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
            errorMessage = error.localizedDescription
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
    @Published public var operationResult: String?

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            async let d = client.doctor()
            async let p = client.profileList()
            let report = try await d
            harnesses = report.harnesses
            profiles = try await p
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func install(harness: String, profile: String, dryRun: Bool) async {
        do {
            operationResult = try await client.install(harness: harness, profile: profile, dryRun: dryRun)
            await refresh()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    public func uninstall(harness: String, dryRun: Bool) async {
        do {
            operationResult = try await client.uninstall(harness: harness, dryRun: dryRun)
            await refresh()
        } catch {
            errorMessage = error.localizedDescription
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
    public var autoPrefs: UserDefaultsAutoUpdatePreferenceStore

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

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
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
