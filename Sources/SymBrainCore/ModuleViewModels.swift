// ModuleViewModels — view models for the embedded Memory and Vault modules.

#if os(macOS)
import AppKit
import Foundation
import SymairaCLIRunner

// MARK: - Memory

/// Scope filter for the Memory module. `all` omits the CLI's `-s` flag.
public enum MemoryScopeFilter: String, CaseIterable, Sendable, Identifiable {
    case all
    case global
    case project
    case agent
    case user
    case session

    public var id: String { rawValue }

    public var label: String {
        self == .all ? "All Scopes" : rawValue.capitalized
    }

    /// The value passed to `-s`, or nil when no filter should be applied.
    public var cliValue: String? {
        self == .all ? nil : rawValue
    }
}

@MainActor
public final class MemoryViewModel: ObservableObject {
    @Published public var versionInfo: VersionInfo?
    @Published public var memories: [MemoryRecord] = []
    @Published public var rules: [MemoryRule] = []
    @Published public var queryLog: MemoryQueryLog?
    @Published public var doctorReport: String?
    @Published public var brokerActivity: [AuditEntry] = []

    @Published public var searchText = ""
    @Published public var scope: MemoryScopeFilter = .all
    @Published public var selectedMemoryID: String?

    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var isBinaryNotFound = false
    @Published public var statusMessage: String?

    private let client: MemoryClient
    private let auditReader = AuditLogReader()

    public init(client: MemoryClient = MemoryClient()) {
        self.client = client
    }

    public var isInstalled: Bool { client.isInstalled }

    public var selectedMemory: MemoryRecord? {
        memories.first { $0.id == selectedMemoryID }
    }

    /// Loads everything the Memory screen shows. Individual sections degrade
    /// on their own so one failing command does not blank the whole screen.
    public func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        guard client.isInstalled else {
            isBinaryNotFound = true
            errorMessage = "The “symmemory” command could not be found. Install it with "
                + "`brew install danieljustus/tap/symmemory`."
            return
        }

        versionInfo = try? await client.version()
        await loadMemories()
        await loadRules()
        await loadQueryLog()
        loadActivity()
    }

    public func loadMemories() async {
        clearError()
        do {
            let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
            if query.isEmpty {
                memories = try await client.list(scope: scope.cliValue)
            } else {
                memories = try await client
                    .search(query: query, scope: scope.cliValue, limit: 50)
                    .map(\.memory)
            }
            if let selectedMemoryID, !memories.contains(where: { $0.id == selectedMemoryID }) {
                self.selectedMemoryID = nil
            }
        } catch {
            report(error)
        }
    }

    public func loadRules() async {
        do {
            rules = try await client.rules()
        } catch {
            // Non-fatal: rules are one tab of several.
            rules = []
        }
    }

    public func loadQueryLog() async {
        do {
            queryLog = try await client.queryLog(limit: 100)
        } catch {
            queryLog = nil
        }
    }

    /// Reads the symbrain broker audit log and keeps the memory server's calls.
    public func loadActivity() {
        brokerActivity = auditReader.read(profile: nil, limit: 500)
            .filter { $0.server == "memory" }
    }

    public func runDoctor() async {
        isLoading = true
        defer { isLoading = false }
        do {
            doctorReport = try await client.doctor()
        } catch {
            report(error)
        }
    }

    @discardableResult
    public func addMemory(content: String, scope: String, kind: String?) async -> Bool {
        clearError()
        do {
            try await client.set(content: content, scope: scope, kind: kind)
            statusMessage = "Memory saved."
            await loadMemories()
            return true
        } catch {
            report(error)
            return false
        }
    }

    @discardableResult
    public func deleteMemory(id: String) async -> Bool {
        clearError()
        do {
            try await client.delete(id: id)
            statusMessage = "Memory deleted."
            if selectedMemoryID == id { selectedMemoryID = nil }
            await loadMemories()
            return true
        } catch {
            report(error)
            return false
        }
    }

    public func copyToPasteboard(_ value: String, label: String) {
        writeToPasteboard(value)
        statusMessage = "\(label) copied to clipboard."
    }

    private func clearError() {
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
    }

    private func report(_ error: Error) {
        let friendly = formatError(error)
        errorMessage = friendly.message
        errorDetail = friendly.detail
        if let cliError = error as? CLIRunnerError, case .binaryNotFound = cliError {
            isBinaryNotFound = true
        }
    }
}

// MARK: - Vault

@MainActor
public final class VaultViewModel: ObservableObject {
    @Published public var availability: VaultAvailability = .checking
    @Published public var versionLine: String?
    @Published public var entries: [VaultEntrySummary] = []
    @Published public var selectedPath: String?
    @Published public var detail: VaultEntryDetail?
    @Published public var revealedFields: Set<String> = []
    @Published public var brokerActivity: [AuditEntry] = []

    @Published public var searchText = ""
    @Published public var passphrase = ""
    @Published public var sessionTTL = "15m"

    @Published public var isLoading = false
    @Published public var isUnlocking = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var statusMessage: String?

    private let client: VaultClient
    private let auditReader = AuditLogReader()

    public init(client: VaultClient = VaultClient()) {
        self.client = client
    }

    public var isInstalled: Bool { client.isInstalled }

    public var isReady: Bool { availability == .ready }

    public var homebrewCommand: String { "brew install \(VaultClient.homebrewFormula)" }

    /// Entries grouped by their first path component, for the sidebar list.
    public var groupedEntries: [(group: String, entries: [VaultEntrySummary])] {
        Dictionary(grouping: entries, by: \.group)
            .map { (group: $0.key, entries: $0.value.sorted { $0.path < $1.path }) }
            .sorted { $0.group < $1.group }
    }

    public func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        availability = await client.availability()
        loadActivity()

        switch availability {
        case .ready:
            versionLine = try? await client.version()
            await loadEntries()
        case .missing, .locked, .checking, .failed:
            entries = []
            detail = nil
            selectedPath = nil
        }
    }

    public func loadEntries() async {
        clearError()
        do {
            let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
            entries = query.isEmpty
                ? try await client.list()
                : try await client.find(query: query)
            if let selectedPath, !entries.contains(where: { $0.path == selectedPath }) {
                self.selectedPath = nil
                detail = nil
            }
        } catch {
            report(error)
        }
    }

    /// Reads the symbrain broker audit log and keeps the vault server's calls.
    public func loadActivity() {
        brokerActivity = auditReader.read(profile: nil, limit: 500)
            .filter { $0.server == "vault" }
    }

    public func unlock() async {
        guard !passphrase.isEmpty else {
            errorMessage = "Enter your vault passphrase to unlock."
            return
        }
        isUnlocking = true
        clearError()
        defer { isUnlocking = false }

        do {
            try await client.unlock(passphrase: passphrase, ttl: sessionTTL)
            passphrase = ""
            statusMessage = "Vault unlocked for \(sessionTTL)."
            await refresh()
        } catch {
            passphrase = ""
            report(error)
        }
    }

    public func lock() async {
        clearError()
        do {
            try await client.lock()
            entries = []
            detail = nil
            selectedPath = nil
            revealedFields = []
            statusMessage = "Vault locked."
            await refresh()
        } catch {
            report(error)
        }
    }

    public func select(path: String) async {
        selectedPath = path
        detail = nil
        revealedFields = []
        clearError()
        do {
            detail = try await client.entry(path: path)
        } catch {
            report(error)
        }
    }

    public func toggleReveal(field: String) {
        if revealedFields.contains(field) {
            revealedFields.remove(field)
        } else {
            revealedFields.insert(field)
        }
    }

    public func isRevealed(field: String) -> Bool {
        revealedFields.contains(field)
    }

    /// Copies a field value. Sensitive fields are marked concealed so
    /// clipboard managers do not archive the secret.
    public func copyToPasteboard(_ value: String, label: String, concealed: Bool = false) {
        writeToPasteboard(value, concealed: concealed)
        statusMessage = "\(label) copied to clipboard."
    }

    private func clearError() {
        errorMessage = nil
        errorDetail = nil
    }

    private func report(_ error: Error) {
        let friendly = formatError(error)
        errorMessage = friendly.message
        errorDetail = friendly.detail
    }
}

// MARK: - Skills

@MainActor
public final class SkillsViewModel: ObservableObject {
    @Published public var versionLine: String?
    @Published public var library: SkillLibrary?
    @Published public var statusReport: SkillStatusReport?
    @Published public var targetsReport: SkillTargetsReport?
    @Published public var logEntries: [SkillLogEntry] = []
    @Published public var doctorReport: SkillsDoctorReport?
    @Published public var brokerActivity: [AuditEntry] = []

    @Published public var searchText = ""
    @Published public var targetFilter = "all"
    @Published public var selectedSkillName: String?
    /// The plan from the last `sync --dry-run`, shown before a live sync.
    @Published public var syncPlan: SkillSyncReport?
    @Published public var syncResult: SkillSyncReport?

    @Published public var isLoading = false
    @Published public var isSyncing = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var isBinaryNotFound = false
    @Published public var statusMessage: String?

    private let client: SkillsClient
    private let auditReader = AuditLogReader()

    public init(client: SkillsClient = SkillsClient()) {
        self.client = client
    }

    public var isInstalled: Bool { client.isInstalled }

    /// Library skills narrowed by the search field (name or description).
    public var skills: [SkillSummary] {
        let all = library?.skills ?? []
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return all }
        return all.filter {
            $0.name.lowercased().contains(query)
                || ($0.description ?? "").lowercased().contains(query)
        }
    }

    public var selectedSkill: SkillSummary? {
        library?.skills.first { $0.name == selectedSkillName }
    }

    /// The harnesses `symskills` knows about, for the target picker.
    public var targetOptions: [String] {
        ["all"] + (targetsReport?.rows.map(\.target) ?? [])
    }

    public func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        guard client.isInstalled else {
            isBinaryNotFound = true
            errorMessage = "The “symskills” command could not be found. Install it with "
                + "`brew install danieljustus/tap/symskills`."
            return
        }

        versionLine = try? await client.version()
        await loadLibrary()
        await loadStatus()
        await loadTargets()
        await loadLog()
        loadActivity()
    }

    public func loadLibrary() async {
        do {
            library = try await client.library()
            if let selectedSkillName,
               !(library?.skills.contains { $0.name == selectedSkillName } ?? false) {
                self.selectedSkillName = nil
            }
        } catch {
            report(error)
        }
    }

    public func loadStatus() async {
        do {
            statusReport = try await client.status(target: targetFilter)
        } catch {
            report(error)
        }
    }

    public func loadTargets() async {
        do {
            targetsReport = try await client.targets()
        } catch {
            // Non-fatal: targets are one tab of several.
            targetsReport = nil
        }
    }

    public func loadLog() async {
        do {
            logEntries = try await client.log().sorted { $0.ts > $1.ts }
        } catch {
            logEntries = []
        }
    }

    /// Reads the symbrain broker audit log and keeps the skills server's calls.
    public func loadActivity() {
        brokerActivity = auditReader.read(profile: nil, limit: 500)
            .filter { $0.server == "skills" }
    }

    public func runDoctor() async {
        isLoading = true
        clearError()
        defer { isLoading = false }
        do {
            doctorReport = try await client.doctor()
        } catch {
            report(error)
        }
    }

    /// Runs `sync --dry-run` and keeps the plan. Nothing is written.
    public func previewSync() async {
        isSyncing = true
        clearError()
        syncResult = nil
        defer { isSyncing = false }
        do {
            syncPlan = try await client.sync(dryRun: true, target: targetFilter)
            statusMessage = "Preview: \(syncPlan?.rows.count ?? 0) planned actions."
        } catch {
            report(error)
        }
    }

    /// Runs a live `sync`, writing rendered skills into the harness roots.
    /// Callers must confirm with the user first — this rewrites files outside
    /// SymBrain's own data directory.
    public func applySync() async {
        isSyncing = true
        clearError()
        defer { isSyncing = false }
        do {
            syncResult = try await client.sync(dryRun: false, target: targetFilter)
            syncPlan = nil
            statusMessage = "Synced \(syncResult?.rows.count ?? 0) installs."
            await loadStatus()
            await loadLog()
        } catch {
            report(error)
        }
    }

    public func clearSyncPlan() {
        syncPlan = nil
        syncResult = nil
    }

    public func copyToPasteboard(_ value: String, label: String) {
        writeToPasteboard(value)
        statusMessage = "\(label) copied to clipboard."
    }

    private func clearError() {
        errorMessage = nil
        errorDetail = nil
        isBinaryNotFound = false
    }

    private func report(_ error: Error) {
        let friendly = formatError(error)
        errorMessage = friendly.message
        errorDetail = friendly.detail
        if let cliError = error as? CLIRunnerError, case .binaryNotFound = cliError {
            isBinaryNotFound = true
        }
    }
}

// MARK: - Pasteboard

/// Writes a value to the general pasteboard.
///
/// Concealed values are additionally marked with the `org.nspasteboard`
/// convention so clipboard managers skip recording them — vault secrets must
/// not end up in a clipboard history.
func writeToPasteboard(_ value: String, concealed: Bool = false) {
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    pasteboard.setString(value, forType: .string)
    if concealed {
        pasteboard.setData(Data(), forType: .init("org.nspasteboard.ConcealedType"))
    }
}
#endif
