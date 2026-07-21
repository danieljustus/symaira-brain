// ViewModels — @MainActor ObservableObject view models for the SymBrain screens.

#if os(macOS)
import Foundation
import SymairaToolKit
import SymairaUpdateCheck

// MARK: - DashboardViewModel

@MainActor
public final class DashboardViewModel: ObservableObject {
    @Published public var versionInfo: VersionInfo?
    @Published public var doctorReport: DoctorReport?
    @Published public var isLoading = false
    @Published public var errorMessage: String?

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            async let v = client.version()
            async let d = client.doctor()
            versionInfo = try await v
            doctorReport = try await d
        } catch {
            errorMessage = error.localizedDescription
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

    private let client: SymBrainClient

    public init(client: SymBrainClient) {
        self.client = client
    }

    public func refresh() async {
        isLoading = true
        defer { isLoading = false }

        versionInfo = try? await client.version()
    }

    public func checkForUpdate() async {
        let checker = UpdateChecker(owner: "danieljustus", repo: "symaira-brain")
        guard let version = versionInfo else { return }
        do {
            if let release = try await checker.check(currentVersion: version.version) {
                updateInfo = "New version available: \(release.tagName)"
            } else {
                updateInfo = "You are up to date (\(version.version))"
            }
        } catch {
            updateInfo = "Update check failed: \(error.localizedDescription)"
        }
    }
}
#endif
