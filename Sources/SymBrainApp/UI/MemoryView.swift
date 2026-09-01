import SwiftUI
import SymairaTheme
import SymBrainCore

/// The memory core built into symbrain: browse, search and add memories,
/// inspect rules, and read both the memory query log and the broker audit
/// trail for the memory server.
struct MemoryView: View {
    @StateObject private var vm: MemoryViewModel
    @State private var tab: Tab = .memories
    @State private var showAddSheet = false
    @State private var memoryToDelete: MemoryRecord?

    /// Memory runs inside the same binary the rest of the app drives, so it
    /// is handed the same client — a Settings binary-path override reaches
    /// this screen too.
    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: MemoryViewModel(client: MemoryClient(brain: client)))
    }

    enum Tab: String, CaseIterable, Identifiable {
        case memories = "Memories"
        case rules = "Rules"
        case queryLog = "Query Log"
        case activity = "Broker Activity"
        case health = "Health"

        var id: String { rawValue }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            headerSection

            switch vm.availability {
            case .checking:
                SymairaLoadingState("Opening the memory store\u{2026}")

            case .missing:
                missingRuntimeSection

            case .failed(let message):
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    SymairaNotice(title: "Memory unavailable", message: message, tone: .critical)
                    Button(action: { Task { await vm.refresh() } }) {
                        Label("Try Again", systemImage: "arrow.clockwise")
                    }
                    .symairaButtonStyle(.primary)
                    .accessibilityLabel("Try Again")
                }

            case .ready:
                ModuleTabStrip(selection: $tab)

                if let error = vm.errorMessage {
                    VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                        SymairaNotice(title: "Error", message: error, tone: .critical)
                        if let detail = vm.errorDetail {
                            SymairaNotice(title: "Details", message: detail, tone: .neutral)
                        }
                    }
                }

                switch tab {
                case .memories: memoriesSection
                case .rules: rulesSection
                case .queryLog: queryLogSection
                case .activity: activitySection
                case .health: healthSection
                }
            }
        }
        .padding(SymairaSpacing.xLarge)
        .task {
            await vm.refresh()
        }
        .sheet(isPresented: $showAddSheet) {
            AddMemorySheet { content, scope, kind in
                Task { await vm.addMemory(content: content, scope: scope, kind: kind) }
                showAddSheet = false
            } onCancel: {
                showAddSheet = false
            }
        }
        .alert(
            "Delete Memory",
            isPresented: Binding(
                get: { memoryToDelete != nil },
                set: { if !$0 { memoryToDelete = nil } }
            )
        ) {
            Button("Cancel", role: .cancel) { memoryToDelete = nil }
            Button("Delete", role: .destructive) {
                if let memory = memoryToDelete {
                    Task { await vm.deleteMemory(id: memory.id) }
                }
                memoryToDelete = nil
            }
        } message: {
            Text("This permanently removes the memory from the local store. This cannot be undone.")
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Memory")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Persistent facts, rules and retrieval log — built into SymBrain")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()

            if let status = vm.statusMessage {
                SymairaBadge(status, tone: .positive)
            }

            Button(action: { showAddSheet = true }) {
                Label("New Memory", systemImage: "plus")
            }
            .symairaButtonStyle(.primary)
            .accessibilityLabel("New Memory")
            .disabled(vm.availability != .ready)

            Button(action: { Task { await vm.refresh() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.secondary)
            .accessibilityLabel("Refresh")
        }
    }

    private var missingRuntimeSection: some View {
        SymairaEmptyState(
            systemImage: "brain",
            title: "SymBrain CLI not found",
            message: "Memory runs inside symbrain. Install the symbrain CLI, then reload this screen."
        ) {
            VStack(spacing: SymairaSpacing.medium) {
                HStack(spacing: SymairaSpacing.small) {
                    Text(vm.homebrewCommand)
                        .font(.callout.monospaced())
                        .foregroundStyle(SymairaTheme.goldPrimary)
                        .textSelection(.enabled)
                    Button {
                        vm.copyToPasteboard(vm.homebrewCommand, label: "Install command")
                    } label: {
                        Image(systemName: "doc.on.doc")
                    }
                    .buttonStyle(.plain)
                    .help("Copy command")
                }
                Button(action: { Task { await vm.refresh() } }) {
                    Label("Check Again", systemImage: "arrow.clockwise")
                }
                .symairaButtonStyle(.primary)
                .accessibilityLabel("Check Again")
            }
        }
    }

    // MARK: - Memories

    private var memoriesSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search memories…", text: $vm.searchText)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await vm.loadMemories() } }

                Picker("Scope", selection: $vm.scope) {
                    ForEach(MemoryScopeFilter.allCases) { item in
                        Text(item.label).tag(item)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 160)
                .onChange(of: vm.scope) { _, _ in
                    Task { await vm.loadMemories() }
                }

                Button(action: { Task { await vm.loadMemories() } }) {
                    Label("Search", systemImage: "magnifyingglass")
                }
                .symairaButtonStyle(.secondary)
                .accessibilityLabel("Search")
            }

            if vm.isLoading && vm.memories.isEmpty {
                SymairaLoadingState("Loading memories…")
            } else if vm.memories.isEmpty {
                SymairaEmptyState(
                    systemImage: "tray",
                    title: "No Memories",
                    message: "Nothing stored for this scope yet. Add one, or let an agent write "
                        + "through the memory MCP server."
                )
            } else {
                HSplitView {
                    memoryList
                    memoryDetail
                }

                if let note = vm.listTruncationNote {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(SymairaTheme.textMuted)
                }
            }
        }
    }

    /// Keyed on `listGeneration` so a changed result set mounts a fresh table
    /// rather than diffing the mounted one, which is the path that re-enters
    /// AppKit's row-span cache (#231). The wrapper keeps the split view's own
    /// child identity — and so its divider position — stable across that
    /// replacement.
    private var memoryList: some View {
        ZStack {
            keyedMemoryList
        }
        .frame(minWidth: 320, idealWidth: 420)
    }

    private var keyedMemoryList: some View {
        List {
            ForEach(vm.memories) { memory in
                Button {
                    vm.selectedMemoryID = memory.id
                } label: {
                    VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                        Text(memory.content)
                            .font(.callout)
                            .foregroundStyle(SymairaTheme.textPrimary)
                            .lineLimit(3)
                        HStack(spacing: SymairaSpacing.small) {
                            SymairaBadge(memory.scope, tone: scopeTone(memory.scope))
                            SymairaBadge(memory.displayTier, tone: .neutral)
                            Text(memory.formattedCreatedAt)
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(SymairaTheme.textMuted)
                        }
                    }
                    .padding(.vertical, SymairaSpacing.xSmall)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Memory: \(memory.content)")
                .listRowBackground(
                    vm.selectedMemoryID == memory.id
                        ? SymairaTheme.bgCardHover
                        : Color.clear
                )
                .contextMenu {
                    Button("Copy Content") {
                        vm.copyToPasteboard(memory.content, label: "Memory")
                    }
                    Button("Copy ID") {
                        vm.copyToPasteboard(memory.id, label: "Memory ID")
                    }
                    Divider()
                    Button("Delete…", role: .destructive) { memoryToDelete = memory }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .id(vm.listGeneration)
    }

    @ViewBuilder
    private var memoryDetail: some View {
        if let memory = vm.selectedMemory {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    Text(memory.content)
                        .font(.body)
                        .foregroundStyle(SymairaTheme.textPrimary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)

                    detailRow("ID", memory.id)
                    detailRow("Scope", memory.scope)
                    detailRow("Tier", memory.displayTier)
                    detailRow("Created", memory.formattedCreatedAt)
                    if let author = memory.createdBy { detailRow("Author", author) }
                    if let review = memory.reviewStatus { detailRow("Review", review) }
                    if let consolidation = memory.consolidationStatus {
                        detailRow("Consolidation", consolidation)
                    }
                    if let accessCount = memory.accessCount {
                        detailRow("Recalled", "\(accessCount)×")
                    }
                    if let expiresAt = memory.expiresAt {
                        detailRow("Expires", formatModuleTimestamp(expiresAt))
                    }

                    if !memory.metadataPairs.isEmpty {
                        Text("Metadata")
                            .font(.headline)
                            .foregroundStyle(SymairaTheme.textPrimary)
                            .padding(.top, SymairaSpacing.small)
                        ForEach(memory.metadataPairs, id: \.key) { pair in
                            detailRow(pair.key, pair.value)
                        }
                    }

                    HStack {
                        Button(action: { vm.copyToPasteboard(memory.content, label: "Memory") }) {
                            Label("Copy", systemImage: "doc.on.doc")
                        }
                        .symairaButtonStyle(.secondary)
                        .accessibilityLabel("Copy")
                        Button(role: .destructive, action: { memoryToDelete = memory }) {
                            Label("Delete", systemImage: "trash")
                        }
                        .symairaButtonStyle(.secondary)
                        .accessibilityLabel("Delete")
                    }
                    .padding(.top, SymairaSpacing.small)
                }
                .padding(SymairaSpacing.large)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minWidth: 300)
        } else {
            SymairaEmptyState(
                systemImage: "sidebar.right",
                title: "No Memory Selected",
                message: "Select an entry to see its metadata and provenance."
            )
            .frame(minWidth: 300)
        }
    }

    // MARK: - Rules

    private var rulesSection: some View {
        Group {
            if vm.rules.isEmpty {
                SymairaEmptyState(
                    systemImage: "list.bullet.rectangle",
                    title: "No Rules",
                    message: "Procedural rules stored in the memory core appear here."
                )
            } else {
                Table(vm.rules) {
                    TableColumn("Scope") { rule in
                        SymairaBadge(rule.scope, tone: scopeTone(rule.scope))
                    }
                    .width(min: 80, ideal: 100)

                    TableColumn("Rule") { rule in
                        Text(rule.content)
                            .font(.callout)
                            .foregroundStyle(SymairaTheme.textPrimary)
                    }

                    TableColumn("Created") { rule in
                        Text(rule.formattedCreatedAt)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 140, ideal: 160)
                }
            }
        }
    }

    // MARK: - Query log

    private var queryLogSection: some View {
        Group {
            if let log = vm.queryLog, log.totalQueries > 0 {
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    HStack(spacing: SymairaSpacing.small) {
                        SymairaBadge("\(log.totalQueries) queries", tone: .informative)
                        ForEach(log.toolCounts.prefix(4), id: \.name) { entry in
                            SymairaBadge("\(entry.name): \(entry.count)", tone: .neutral)
                        }
                        ForEach(log.actorCounts.prefix(2), id: \.name) { entry in
                            SymairaBadge("\(entry.name): \(entry.count)", tone: .positive)
                        }
                    }

                    Table(log.entries) {
                        TableColumn("Time") { entry in
                            Text(entry.formattedCreatedAt)
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(SymairaTheme.textSecondary)
                        }
                        .width(min: 140, ideal: 160)

                        TableColumn("Actor") { entry in
                            Text(entry.actor ?? "—")
                                .font(.caption.monospaced())
                                .foregroundStyle(SymairaTheme.textMuted)
                        }
                        .width(min: 90, ideal: 120)

                        TableColumn("Tool") { entry in
                            SymairaBadge(entry.tool, tone: .informative)
                        }
                        .width(min: 120, ideal: 150)

                        TableColumn("Query") { entry in
                            Text(entry.queryText ?? "—")
                                .font(.caption)
                                .foregroundStyle(SymairaTheme.textPrimary)
                        }

                        TableColumn("Duration") { entry in
                            Text(entry.durationMs.map { "\($0)ms" } ?? "—")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(SymairaTheme.textMuted)
                        }
                        .width(min: 70, ideal: 80)
                    }
                }
            } else {
                SymairaEmptyState(
                    systemImage: "chart.bar.doc.horizontal",
                    title: "No Queries Logged",
                    message: "The memory query log records every MCP retrieval. It fills up once "
                        + "an agent searches the store."
                )
            }
        }
    }

    // MARK: - Broker activity

    private var activitySection: some View {
        ModuleActivityTable(
            entries: vm.brokerActivity,
            emptyMessage: "Calls routed to the memory server through symbrain appear here.",
            onRefresh: { Task { await vm.loadActivity() } }
        )
    }

    // MARK: - Health

    private var healthSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Button(action: { Task { await vm.runDoctor() } }) {
                Label("Run symbrain doctor", systemImage: "stethoscope")
            }
            .symairaButtonStyle(.primary)
            .accessibilityLabel("Run symbrain doctor")

            if vm.isLoading {
                SymairaLoadingState("Running health checks…")
            } else if let report = vm.doctorReport {
                ScrollView {
                    Text(report)
                        .font(.caption.monospaced())
                        .foregroundStyle(SymairaTheme.textPrimary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(SymairaSpacing.medium)
                }
                .background(
                    Color.white.opacity(0.04),
                    in: RoundedRectangle(cornerRadius: SymairaRadius.control)
                )
            } else {
                SymairaEmptyState(
                    systemImage: "stethoscope",
                    title: "No Report Yet",
                    message: "Run the doctor to check the database, embedding backend and "
                        + "configuration."
                )
            }
        }
    }

    // MARK: - Helpers

    private func detailRow(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top, spacing: SymairaSpacing.small) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textSecondary)
                .frame(width: 110, alignment: .leading)
            Text(value)
                .font(.caption.monospaced())
                .foregroundStyle(SymairaTheme.textPrimary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func scopeTone(_ scope: String) -> SymairaTone {
        switch scope {
        case "global": .informative
        case "project": .positive
        case "user": .warning
        case "session": .neutral
        default: .neutral
        }
    }
}

// MARK: - Add sheet

private struct AddMemorySheet: View {
    let onSave: (String, String, String?) -> Void
    let onCancel: () -> Void

    @State private var content = ""
    @State private var scope = "global"
    @State private var kind = "none"

    private let scopes = ["global", "project", "agent", "user", "session"]
    private let kinds = ["none", "user", "feedback", "project", "reference"]

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            Text("New Memory")
                .font(.title2.bold())
                .foregroundStyle(SymairaTheme.textPrimary)

            TextEditor(text: $content)
                .font(.body)
                .frame(minHeight: 140)
                .scrollContentBackground(.hidden)
                .padding(SymairaSpacing.small)
                .background(
                    Color.white.opacity(0.05),
                    in: RoundedRectangle(cornerRadius: SymairaRadius.control)
                )

            HStack(spacing: SymairaSpacing.large) {
                Picker("Scope", selection: $scope) {
                    ForEach(scopes, id: \.self) { Text($0.capitalized).tag($0) }
                }
                .frame(width: 200)

                Picker("Kind", selection: $kind) {
                    ForEach(kinds, id: \.self) { Text($0.capitalized).tag($0) }
                }
                .frame(width: 200)
            }

            HStack {
                Spacer()
                Button("Cancel", action: onCancel)
                    .symairaButtonStyle(.secondary)
                Button("Save") {
                    onSave(content, scope, kind == "none" ? nil : kind)
                }
                .symairaButtonStyle(.primary)
                .disabled(content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(SymairaSpacing.xLarge)
        .frame(width: 560)
        .background(SymairaTheme.bgDark)
    }
}

// MARK: - Shared activity table

/// Renders symbrain broker audit entries for one server. Both module screens
/// use it so their "Broker Activity" tabs stay identical.
struct ModuleActivityTable: View {
    let entries: [AuditEntry]
    let emptyMessage: String
    let onRefresh: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack {
                Text("\(entries.count) recorded calls")
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textSecondary)
                Spacer()
                Button(action: onRefresh) {
                    Label("Reload", systemImage: "arrow.clockwise")
                }
                .symairaButtonStyle(.secondary)
                .accessibilityLabel("Reload")
            }

            if entries.isEmpty {
                SymairaEmptyState(
                    systemImage: "doc.text.magnifyingglass",
                    title: "No Broker Activity",
                    message: emptyMessage
                )
            } else {
                Table(entries) {
                    TableColumn("Timestamp") { entry in
                        Text(entry.formattedTime)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textSecondary)
                    }
                    .width(min: 140, ideal: 160)

                    TableColumn("Profile") { entry in
                        Text(entry.profile)
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 80, ideal: 100)

                    TableColumn("Tool") { entry in
                        Text(entry.tool)
                            .font(.caption.monospaced())
                            .foregroundStyle(SymairaTheme.textPrimary)
                    }
                    .width(min: 160, ideal: 200)

                    TableColumn("Status") { entry in
                        SymairaBadge(entry.status, tone: statusTone(entry.status))
                    }
                    .width(min: 60, ideal: 70)

                    TableColumn("Duration") { entry in
                        Text("\(entry.durationMs)ms")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 60, ideal: 70)
                }
            }
        }
    }

    private func statusTone(_ status: String) -> SymairaTone {
        switch status {
        case "ok": .positive
        case "error": .critical
        case "timeout": .warning
        default: .neutral
        }
    }
}

// MARK: - Shared module tab strip (#309)

/// Pure-SwiftUI segmented tab strip for module screens.
///
/// Native AppKit segmented pickers (`Picker(...).pickerStyle(.segmented)`)
/// conflict with selectable `List` and `Table` coordinators in AppKit hosting,
/// causing tab clicks to be dropped or the tab strip to become inert (#309).
/// This component uses pure SwiftUI buttons with explicit selection mutation.
struct ModuleTabStrip<T: CaseIterable & Identifiable & Hashable & RawRepresentable>: View where T.AllCases: RandomAccessCollection, T.RawValue == String {
    @Binding var selection: T
    let tabs: [T]

    init(selection: Binding<T>, tabs: [T] = Array(T.allCases)) {
        self._selection = selection
        self.tabs = tabs
    }

    var body: some View {
        HStack(spacing: 2) {
            ForEach(tabs) { tab in
                let isSelected = selection == tab
                Button {
                    selection = tab
                } label: {
                    Text(tab.rawValue)
                        .font(.callout.weight(isSelected ? .semibold : .regular))
                        .foregroundStyle(isSelected ? SymairaTheme.textPrimary : SymairaTheme.textSecondary)
                        .padding(.horizontal, SymairaSpacing.medium)
                        .padding(.vertical, 5)
                        .background {
                            if isSelected {
                                RoundedRectangle(cornerRadius: 6, style: .continuous)
                                    .fill(SymairaTheme.bgCardHover)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                                            .stroke(SymairaTheme.goldPrimary.opacity(0.35), lineWidth: 1)
                                    )
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(tab.rawValue)
                .accessibilityAddTraits(isSelected ? [.isSelected] : [])
            }
        }
        .padding(3)
        .background(
            Color.white.opacity(0.04),
            in: RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
                .stroke(SymairaTheme.borderGlass, lineWidth: 1)
        )
    }
}

