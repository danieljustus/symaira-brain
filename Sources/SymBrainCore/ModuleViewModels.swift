// ModuleViewModels — view models for the embedded Memory and Vault modules.

#if os(macOS)
import AppKit
import CryptoKit
import Foundation

// MARK: - Runtime availability (Memory / Skills)

/// Unified runtime-state enum for module view models that need a
/// checking / missing / ready / failed lifecycle, matching
/// VaultAvailability's shape but without the vault-specific `.locked` case.
public enum RuntimeAvailability: Sendable, Equatable {
    /// Not determined yet.
    case checking
    /// The CLI binary is not installed.
    case missing
    /// Installed and reachable — commands can run.
    case ready
    /// Installed, but the check itself failed.
    case failed(String)
}

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
public final class MemoryViewModel: ObservableObject, ModuleViewModelProtocol {
    @Published public var memories: [MemoryRecord] = [] {
        didSet {
            if memories.map(\.id) != oldValue.map(\.id) { listGeneration += 1 }
        }
    }
    @Published public var rules: [MemoryRule] = []
    @Published public var queryLog: MemoryQueryLog?
    @Published public var doctorReport: String?
    @Published public var brokerActivity: [AuditEntry] = []

    @Published public var searchText = ""
    @Published public var scope: MemoryScopeFilter = .all
    @Published public var selectedMemoryID: String?

    /// How many memories the store holds for the current scope, before the
    /// list is bounded to `listPageSize`.
    @Published public var totalMemoryCount = 0

    /// Set when the last load was a search that came back with exactly as many
    /// hits as it asked for, so further matches may exist that were not
    /// fetched. The search command reports no total, so the count of hidden
    /// matches is unknowable without another query — only their possibility.
    @Published public var searchMayHaveMoreMatches = false

    @Published public var isLoading = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var isBinaryNotFound = false
    @Published public var availability: RuntimeAvailability = .checking
    @Published public var statusMessage: String?

    private let client: MemoryClient
    private let auditReader = AuditLogReader()

    public init(client: MemoryClient = MemoryClient()) {
        self.client = client
    }

    public var isInstalled: Bool { client.isInstalled }

    /// Memory ships inside symbrain, so the only thing that can be missing
    /// is symbrain itself.
    public var homebrewCommand: String { "brew install danieljustus/tap/symbrain" }

    public var selectedMemory: MemoryRecord? {
        memories.first { $0.id == selectedMemoryID }
    }

    /// The Memory list renders at most this many rows.
    ///
    /// Past roughly this many variable-height rows AppKit switches the backing
    /// table to *estimated* row heights. Its span cache then adjusts a span
    /// mid-pass, resizes the table from inside that same pass, and re-enters
    /// the cache — which AppKit reports as a reentrant table-delegate
    /// operation and has said it will treat as a fatal assert. Rendering a
    /// bounded page keeps every row measured directly, and also stops a large
    /// store from building hundreds of row views for a pane a few dozen rows
    /// tall.
    public static let listPageSize = 100

    /// How many hits a search asks the CLI for.
    public static let searchResultLimit = 50

    /// Changes every time the list's row set changes (#231).
    ///
    /// #230 bounded the rendered page so mounting the list stops reaching
    /// AppKit's estimated-row-height code through row *volume*. A search
    /// reaches the same code by the other route: SwiftUI's list coordinator
    /// applies a row diff to the already-mounted table, and the span cache
    /// re-enters itself from inside that update. Nothing in the row content
    /// avoids it — uniform heights, a smaller page and a fixed row count were
    /// all measured and none helped, because the trigger is the diff itself.
    ///
    /// The view keys the list on this value, so a changed result set replaces
    /// the table instead of diffing it, taking the clean mount path. Every load
    /// here fetches a complete result set rather than appending a page, so a
    /// changed row set always means a new list rather than a scroll position
    /// worth preserving.
    @Published public private(set) var listGeneration = 0

    /// The first `listPageSize` records — what the list actually renders.
    public static func boundedPage(_ all: [MemoryRecord]) -> [MemoryRecord] {
        Array(all.prefix(listPageSize))
    }

    /// True when the store holds more memories for this scope than the list
    /// is showing.
    public var isMemoryListTruncated: Bool {
        totalMemoryCount > memories.count
    }

    /// What to tell the user under the list when it is not showing everything,
    /// or nil when the list is complete.
    ///
    /// Browsing knows the real total and states it. Searching does not — the
    /// command reports no total — so it only says the results start at the
    /// beginning, rather than inventing a number.
    public var listTruncationNote: String? {
        if isMemoryListTruncated {
            return "Showing \(memories.count) of \(totalMemoryCount) memories. "
                + "Search or pick a scope to narrow the list."
        }
        if searchMayHaveMoreMatches {
            return "Showing the first \(memories.count) matches. "
                + "Refine the search to narrow it."
        }
        return nil
    }

    /// Loads everything the Memory screen shows. Individual sections degrade
    /// on their own so one failing command does not blank the whole screen.
    public func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        guard client.isInstalled else {
            isBinaryNotFound = true
            availability = .missing
            errorMessage = "The \u{201C}symbrain\u{201D} command could not be found. Install it with "
                + "`brew install danieljustus/tap/symbrain`."
            return
        }

        availability = .ready
        await loadMemories()
        await loadRules()
        await loadQueryLog()
        await loadActivity()
    }

    public func loadMemories() async {
        clearError()
        do {
            let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
            if query.isEmpty {
                let all = try await client.list(scope: scope.cliValue)
                totalMemoryCount = all.count
                memories = Self.boundedPage(all)
                searchMayHaveMoreMatches = false
            } else {
                let hits = try await client
                    .search(query: query, scope: scope.cliValue, limit: Self.searchResultLimit)
                    .map(\.memory)
                totalMemoryCount = hits.count
                memories = hits
                searchMayHaveMoreMatches = hits.count == Self.searchResultLimit
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
    public func loadActivity() async {
        brokerActivity = await auditReader.read(profile: nil, server: "memory", limit: 500)
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

    // clearError() and report(_:) are provided by ModuleViewModelProtocol.
}

// MARK: - Vault

public enum VaultIntakePhase: Sendable, Equatable {
    case idle
    case previewing
    case reviewing
    case staging
    case promoting
    case done
    case failed(String)
}

@MainActor
public protocol VaultClientProtocol: Sendable {
    var isInstalled: Bool { get }
    func availability(profile: String?) async -> VaultAvailability
    func version(profile: String?) async throws -> String
    func unlock(passphrase: String, ttl: String, profile: String?) async throws
    func lock(profile: String?) async throws
    func list(profile: String?) async throws -> [VaultEntrySummary]
    func find(query: String, profile: String?) async throws -> [VaultEntrySummary]
    func entry(path: String, profile: String?) async throws -> VaultEntryDetail
    func create(path: String, value: String, profile: String?) async throws -> VaultCreateConfirmation
    func create(draft: VaultCredentialDraft, profile: String?) async throws -> VaultCreateConfirmation
    func set(path: String, field: String, value: String, profile: String?) async throws -> VaultSetConfirmation
    func update(path: String, original: VaultEntryDetail, draft: VaultCredentialDraft, profile: String?) async throws -> VaultSetConfirmation
    func delete(path: String, profile: String?) async throws -> VaultDeleteConfirmation
    func generatePassword(length: Int, symbols: Bool, profile: String?) async throws -> String
    func intakePreview(files: [URL], profile: String?) async throws -> VaultIntakeResponse
    func intakeStage(files: [URL], ocrTexts: [URL: URL], moveToTrash: Bool, profile: String?) async throws -> VaultIntakeResponse
    func intakeReviewBatches(profile: String?) async throws -> [String]
    func intakePromote(importID: String, overwrite: Bool, profile: String?) async throws
}

public extension VaultClientProtocol {
    func create(draft: VaultCredentialDraft, profile: String?) async throws -> VaultCreateConfirmation {
        try await create(path: draft.path, value: draft.secret, profile: profile)
    }

    func update(path: String, original: VaultEntryDetail, draft: VaultCredentialDraft, profile: String?) async throws -> VaultSetConfirmation {
        try await set(path: path, field: original.primarySecret?.field ?? "password", value: draft.secret, profile: profile)
    }
}

extension VaultClient: VaultClientProtocol {}

public final class VaultViewModel: ObservableObject, ModuleViewModelProtocol {
    @Published public var availability: VaultAvailability = .checking {
        didSet {
            if availability != .ready { invalidatePendingDetail() }
        }
    }
    @Published public var versionLine: String?
    @Published public var entries: [VaultEntrySummary] = []
    @Published public var selectedPath: String?
    @Published public var detail: VaultEntryDetail?
    @Published public var revealedFields: Set<String> = []
    @Published public var brokerActivity: [AuditEntry] = []

    @Published public var searchText = ""
    @Published public var passphrase = ""
    @Published public var sessionTTL = "15m"
    @Published public var createPath = ""
    @Published public var createValue = ""
    @Published public var createType = "password"
    @Published public var createUsername = ""
    @Published public var createURL = ""
    @Published public var createNotes = ""
    @Published public var createUsageHint = ""
    @Published public var createAutoRotate = false
    @Published public var createExpiresAt = ""
    @Published public var createTOTPSecret = ""
    @Published public var createTOTPIssuer = ""
    @Published public var createTOTPAccount = ""
    @Published public var createConfirmation: VaultCreateConfirmation?
    @Published public var isCreating = false
    @Published public var editValue = ""
    @Published public var editUsername = ""
    @Published public var editURL = ""
    @Published public var editNotes = ""
    @Published public var editConfirmation: VaultSetConfirmation?
    @Published public var isEditing = false
    @Published public var isDeleting = false
    @Published public var deleteConfirmation: VaultDeleteConfirmation?

    @Published public var generatedPassword = ""
    @Published public var isGenerating = false
    @Published public var intakeFiles: [URL] = []
    @Published public var intakePreview: VaultIntakeResponse?
    @Published public var intakeDrafts: [VaultIntakeReviewDraft] = []
    @Published public var intakeImportID: String?
    @Published public var intakePhase: VaultIntakePhase = .idle
    @Published public var intakeReviewComplete = false
    @Published public private(set) var intakeReviewSnapshot: VaultIntakeReviewSnapshot?
    @Published public var intakeMoveToTrash = false

    @Published public var isLoading = false
    @Published public var isUnlocking = false
    @Published public var errorMessage: String?
    @Published public var errorDetail: String?
    @Published public var isBinaryNotFound = false
    @Published public var statusMessage: String?

    private let client: any VaultClientProtocol
    private let auditReader = AuditLogReader()
    private let sleep: @Sendable (Duration) async throws -> Void
    private let clipboardWriter: @MainActor (String, Bool) -> Void
    private var generation = 0
    private var generationTask: Task<Void, Never>?
    private var generationWorkTask: Task<String, Error>?
    private var intakeGeneration = 0
    private var revealTask: Task<Void, Never>?
    private var detailExpiryTask: Task<Void, Never>?

    public init(
        client: any VaultClientProtocol = VaultClient(),
        sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
        clipboardWriter: (@MainActor (String, Bool) -> Void)? = nil
    ) {
        self.client = client
        self.sleep = sleep
        self.clipboardWriter = clipboardWriter ?? { value, concealed in
            writeToPasteboard(value, concealed: concealed)
        }
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
        invalidatePendingDetail()
        isLoading = true
        clearError()
        defer { isLoading = false }

        availability = await client.availability(profile: nil)
        await loadActivity()

        switch availability {
        case .ready:
            versionLine = try? await client.version(profile: nil)
            guard availability == .ready else { return }
            await loadEntries()
        case .missing, .locked, .checking, .failed:
            entries = []
            detail = nil
            selectedPath = nil
        }
    }

    public func generatePassword(length: Int, symbols: Bool) async {
        generationTask?.cancel()
        generationWorkTask?.cancel()
        let requestGeneration = generation
        isGenerating = true
        defer {
            if generation == requestGeneration { isGenerating = false }
            generatedPassword = ""
            generationTask = nil
            generationWorkTask = nil
        }
        generationWorkTask = Task { try await client.generatePassword(length: length, symbols: symbols, profile: nil) }
        let task = generationWorkTask!
        generationTask = Task { @MainActor in
            do {
                let password = try await task.value
                guard !Task.isCancelled, generation == requestGeneration else { return }
                generatedPassword = password
                createValue = password
                statusMessage = "Password generated by the vault service."
            } catch is CancellationError {
                return
            } catch { report(error) }
        }
        await generationTask?.value
    }

    public func setIntakeFiles(_ files: [URL]) {
        invalidateIntake()
        intakeFiles = files
        intakePhase = .idle
    }

    public func previewIntake() async {
        guard !intakeFiles.isEmpty else { return }
        let requestGeneration = intakeGeneration
        let files = intakeFiles
        intakePhase = .previewing; clearError()
        do {
            let response = try await client.intakePreview(files: files, profile: nil)
            guard requestGeneration == intakeGeneration, files == intakeFiles else { return }
            intakePreview = response
            intakeDrafts = response.results.filter(\.isOK).map(VaultIntakeReviewDraft.init)
            intakeImportID = nil
            intakeReviewSnapshot = nil
            intakeReviewComplete = false
            intakePhase = .reviewing
        } catch { if requestGeneration == intakeGeneration { intakePhase = .failed(error.localizedDescription); report(error) } }
    }

    public func stageIntake() async {
        guard !intakeFiles.isEmpty else { return }
        let requestGeneration = intakeGeneration
        let files = intakeFiles
        intakePhase = .staging; clearError()
        do {
            let response = try await client.intakeStage(files: files, ocrTexts: [:], moveToTrash: intakeMoveToTrash, profile: nil)
            guard requestGeneration == intakeGeneration, files == intakeFiles else { return }
            guard let importID = response.importID, !importID.isEmpty else {
                intakePhase = .failed("Vault service did not return an import ID.")
                errorMessage = "Vault service did not return an import ID; promotion is unavailable."
                return
            }
            intakeImportID = importID
            intakePreview = response
            intakeDrafts = response.results.filter(\.isOK).map(VaultIntakeReviewDraft.init)
            intakeReviewSnapshot = nil
            intakeReviewComplete = false
            intakePhase = .reviewing
        } catch { if requestGeneration == intakeGeneration { intakePhase = .failed(error.localizedDescription); report(error) } }
    }

    /// Binds review approval to the exact staged batch, source bytes and
    /// sanitized target snapshot. A Boolean alone is not an authorization.
    public func markIntakeReviewed() {
        guard let importID = intakeImportID, !importID.isEmpty, !intakeDrafts.isEmpty else {
            errorMessage = "Stage an intake batch before reviewing it."
            return
        }
        intakeReviewSnapshot = VaultIntakeReviewSnapshot(
            importID: importID,
            sourceFiles: intakeFiles.map(sourceSnapshot),
            drafts: intakeDrafts
        )
        intakeReviewComplete = true
    }

    public func promoteIntake(overwrite: Bool = false) async {
        guard let snapshot = intakeReviewSnapshot, intakeReviewComplete,
              snapshot.importID == intakeImportID else {
            errorMessage = "Review the staged intake before promoting it."
            return
        }
        guard snapshot.sourceFiles == intakeFiles.map(sourceSnapshot), snapshot.drafts == intakeDrafts else {
            intakeReviewComplete = false
            intakeReviewSnapshot = nil
            errorMessage = "The staged intake changed; review it again before promoting."
            return
        }
        let requestGeneration = intakeGeneration
        intakePhase = .promoting; clearError()
        do {
            try await client.intakePromote(importID: snapshot.importID, overwrite: overwrite, profile: nil)
            guard requestGeneration == intakeGeneration, snapshot == intakeReviewSnapshot else { return }
            intakePhase = .done
            statusMessage = "Intake batch promoted and confirmed by the vault service."
        } catch { if requestGeneration == intakeGeneration { intakePhase = .failed(error.localizedDescription); report(error) } }
    }

    public func clearIntake() {
        invalidateIntake()
    }

    private func invalidateIntake() {
        intakeGeneration &+= 1
        intakeFiles = []
        intakePreview = nil
        intakeDrafts = []
        intakeImportID = nil
        intakeReviewSnapshot = nil
        intakeReviewComplete = false
        intakePhase = .idle
    }

    private func sourceSnapshot(_ url: URL) -> VaultIntakeSourceSnapshot {
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
        let data = (try? Data(contentsOf: url)) ?? Data()
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        return VaultIntakeSourceSnapshot(
            path: url.path,
            size: Int64(values?.fileSize ?? data.count),
            modified: values?.contentModificationDate,
            sha256: digest
        )
    }

    public func createEntry() async {
        let path = createPath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !path.isEmpty, !createValue.isEmpty else {
            errorMessage = "Enter an entry path and secret value."
            return
        }
        isCreating = true
        clearError()
        defer {
            isCreating = false
            createValue = ""
            createTOTPSecret = ""
        }
        do {
            let draft = VaultCredentialDraft(path: path, type: createType, secret: createValue, username: createUsername, url: createURL, notes: createNotes, usageHint: createUsageHint, autoRotate: createAutoRotate, expiresAt: createExpiresAt, totpSecret: createTOTPSecret, totpIssuer: createTOTPIssuer, totpAccount: createTOTPAccount)
            let confirmation = try await client.create(draft: draft, profile: nil)
            createConfirmation = confirmation
            createPath = ""
            statusMessage = "Secret created and confirmed by the vault service."
            await loadEntries()
        } catch {
            report(error)
        }
    }

    public func setSelectedEntry() async {
        guard let path = selectedPath, let field = detail?.primarySecret?.field, detail?.path == path, !editValue.isEmpty else { errorMessage = "Reveal an entry and enter a new secret value."; return }
        isEditing = true; clearError()
        defer { isEditing = false; editValue = "" }
        do {
            let draft = VaultCredentialDraft(path: path, type: detail?.type ?? "password", secret: editValue, username: editUsername, url: editURL, notes: editNotes, usageHint: detail?.usageHint ?? "", autoRotate: detail?.autoRotate ?? false, expiresAt: detail?.expiresAt ?? "")
            editConfirmation = try await client.update(path: path, original: detail!, draft: draft, profile: nil)
            statusMessage = "Secret updated and confirmed by the vault service."
            await loadEntries()
        } catch { report(error) }
    }

    public func deleteEntry(path: String) async {
        isDeleting = true; clearError(); invalidatePendingDetail()
        defer { isDeleting = false }
        do {
            deleteConfirmation = try await client.delete(path: path, profile: nil)
            entries.removeAll { $0.path == path }; selectedPath = nil; detail = nil
            statusMessage = "Secret deleted and absence confirmed by the vault service."
        } catch { report(error) }
    }

    public func loadEntries() async {
        clearError()
        do {
            let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
            entries = query.isEmpty
                ? try await client.list(profile: nil)
                : try await client.find(query: query, profile: nil)
            if let selectedPath, !entries.contains(where: { $0.path == selectedPath }) {
                self.selectedPath = nil
                detail = nil
            }
        } catch {
            report(error)
        }
    }

    /// Reads the symbrain broker audit log and keeps the vault server's calls.
    public func loadActivity() async {
        brokerActivity = await auditReader.read(profile: nil, server: "vault", limit: 500)
    }

    public func unlock() async {
        guard !passphrase.isEmpty else {
            errorMessage = "Enter your vault passphrase to unlock."
            return
        }
        isUnlocking = true
        clearError()
        defer { isUnlocking = false }

        defer { passphrase = "" }
        do {
            try await client.unlock(passphrase: passphrase, ttl: sessionTTL, profile: nil)
            statusMessage = "Vault unlocked for \(sessionTTL)."
            await refresh()
        } catch {
            report(error)
        }
    }

    public func lock() async {
        generation &+= 1
        generationTask?.cancel()
        generationWorkTask?.cancel()
        generationTask = nil
        generationWorkTask = nil
        isGenerating = false
        generatedPassword = ""
        createValue = ""
        invalidateIntake()
        invalidatePendingDetail()
        clearError()
        do {
            try await client.lock(profile: nil)
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

    /// Selects an entry using the metadata-only list result. Secret fields are
    /// not fetched until the user explicitly chooses to reveal this entry.
    public func select(path: String) async {
        invalidatePendingDetail()
        selectedPath = path
        detail = nil
        revealedFields = []
        clearError()
    }

    /// Explicit human action that crosses the plaintext boundary for the
    /// selected entry. The vault remains the storage and authorization owner.
    public func revealSelectedEntry() async {
        guard let selectedPath, availability == .ready else { return }
        invalidateDetailExpiry()
        let requestGeneration = generation
        clearError()
        revealTask?.cancel()
        let task = Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let fetched = try await self.client.entry(path: selectedPath, profile: nil)
                guard !Task.isCancelled, self.generation == requestGeneration,
                      self.selectedPath == selectedPath, self.availability == .ready else { return }
                self.detail = fetched
                self.editUsername = fetched.fields["username"]?.displayString ?? ""
                self.editURL = fetched.fields["url"]?.displayString ?? ""
                self.editNotes = fetched.fields["notes"]?.displayString ?? ""
                self.detailExpiryTask = Task { @MainActor [weak self] in
                    guard let self else { return }
                    do { try await self.sleep(.seconds(30)) } catch { return }
                    guard !Task.isCancelled, self.generation == requestGeneration,
                          self.selectedPath == selectedPath else { return }
                    self.detail = nil
                    self.revealedFields = []
                }
            } catch is CancellationError {
                return
            } catch {
                guard self.generation == requestGeneration, self.selectedPath == selectedPath else { return }
                self.detail = nil
                self.report(error)
            }
        }
        revealTask = task
        await task.value
    }

    private func invalidatePendingDetail() {
        generation &+= 1
        revealTask?.cancel()
        revealTask = nil
        invalidateDetailExpiry()
        detail = nil
        revealedFields = []
    }

    private func invalidateDetailExpiry() {
        detailExpiryTask?.cancel()
        detailExpiryTask = nil
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

    private func copyToPasteboard(
        _ value: String,
        label: String,
        intent: VaultCopyIntent = .ordinary
    ) {
        clipboardWriter(value, intent == .revealedSensitive)
        statusMessage = "\(label) copied to clipboard."
    }

    public func copyInstallCommand() {
        copyToPasteboard(homebrewCommand, label: "Install command")
    }

    public func copyTOTP() {
        guard availability == .ready,
              let selectedPath,
              let detail,
              detail.path == selectedPath,
              let totp = detail.totp else { return }
        copyToPasteboard(totp.code, label: "TOTP code", intent: .revealedSensitive)
    }

    public func copyField(_ field: String) {
        guard availability == .ready,
              let selectedPath,
              let detail,
              detail.path == selectedPath,
              let value = detail.fields[field]?.displayString,
              !value.isEmpty else { return }
        let sensitive = VaultFieldSecurity.isSensitive(field)
        guard !sensitive || revealedFields.contains(field) else { return }
        copyToPasteboard(value, label: field, intent: sensitive ? .revealedSensitive : .ordinary)
    }

    // clearError() and report(_:) are provided by ModuleViewModelProtocol.
}

// MARK: - Skills

@MainActor
public final class SkillsViewModel: ObservableObject, ModuleViewModelProtocol {
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
    @Published public var availability: RuntimeAvailability = .checking
    @Published public var statusMessage: String?

    private let client: SkillsClient
    private let auditReader = AuditLogReader()

    public init(client: SkillsClient = SkillsClient()) {
        self.client = client
    }

    public var isInstalled: Bool { client.isInstalled }

    /// Skills ships inside symbrain, so the only thing that can be missing
    /// is symbrain itself.
    public var homebrewCommand: String { "brew install danieljustus/tap/symbrain" }

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

    /// The harnesses the skill core knows about, for the target picker.
    public var targetOptions: [String] {
        ["all"] + (targetsReport?.rows.map(\.target) ?? [])
    }

    public func refresh() async {
        isLoading = true
        clearError()
        defer { isLoading = false }

        guard client.isInstalled else {
            isBinaryNotFound = true
            availability = .missing
            errorMessage = "The \u{201C}symbrain\u{201D} command could not be found. Install it with "
                + "`brew install danieljustus/tap/symbrain`."
            return
        }

        availability = .ready
        await loadLibrary()
        await loadStatus()
        await loadTargets()
        await loadLog()
        await loadActivity()
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
    public func loadActivity() async {
        brokerActivity = await auditReader.read(profile: nil, server: "skills", limit: 500)
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

    // clearError() and report(_:) are provided by ModuleViewModelProtocol.
}

// MARK: - Pasteboard

/// Small seam for testing clipboard expiry without touching the user's
/// pasteboard. `changeCount` is the ownership token: value equality alone is
/// unsafe because another app may replace the value with the same string.
@MainActor
protocol ClipboardPasteboard: AnyObject {
    var changeCount: Int { get }
    func clearContents()
    func setString(_ string: String, forType type: NSPasteboard.PasteboardType)
    func setData(_ data: Data, forType type: NSPasteboard.PasteboardType)
}

@MainActor
private final class SystemClipboardPasteboard: ClipboardPasteboard {
    let pasteboard = NSPasteboard.general
    var changeCount: Int { pasteboard.changeCount }
    func clearContents() { pasteboard.clearContents() }
    func setString(_ string: String, forType type: NSPasteboard.PasteboardType) {
        pasteboard.setString(string, forType: type)
    }
    func setData(_ data: Data, forType type: NSPasteboard.PasteboardType) {
        pasteboard.setData(data, forType: type)
    }
}

@MainActor
final class ClipboardLifetimeController {
    private let pasteboard: ClipboardPasteboard
    private let sleep: @Sendable (Duration) async throws -> Void
    private var expiryTask: Task<Void, Never>?

    init(
        pasteboard: ClipboardPasteboard,
        sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        self.pasteboard = pasteboard
        self.sleep = sleep
    }

    deinit { expiryTask?.cancel() }

    func write(_ value: String, concealed: Bool, lifetime: Duration = .seconds(30)) {
        expiryTask?.cancel()
        pasteboard.clearContents()
        pasteboard.setString(value, forType: .string)
        if concealed {
            pasteboard.setData(Data(), forType: .init("org.nspasteboard.ConcealedType"))
        }
        guard concealed else { return }
        let expectedChangeCount = pasteboard.changeCount
        let pasteboard = self.pasteboard
        let sleep = self.sleep
        // Deliberately capture only the pasteboard, sleeper, and ownership token.
        // The plaintext value is never retained by the sleeping task.
        expiryTask = Task { @MainActor [pasteboard, expectedChangeCount, sleep] in
            do { try await sleep(lifetime) } catch { return }
            guard !Task.isCancelled, pasteboard.changeCount == expectedChangeCount else { return }
            pasteboard.clearContents()
        }
    }
}

@MainActor private let systemClipboardLifetime = ClipboardLifetimeController(
    pasteboard: SystemClipboardPasteboard()
)

/// Writes a value and bounds its clipboard lifetime. The expiry clears only
/// contents still owned by this write.
@MainActor
func writeToPasteboard(_ value: String, concealed: Bool = false) {
    systemClipboardLifetime.write(value, concealed: concealed)
}
#endif
