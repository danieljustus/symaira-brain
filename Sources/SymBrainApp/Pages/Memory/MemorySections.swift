import SwiftUI
import SymairaTheme
import SymBrainCore

extension MemoryPage {
    // MARK: - Memories

    var memoriesSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search memories", text: $vm.searchText)
                    .textFieldStyle(.symaira)
                    .onSubmit { Task { await vm.loadMemories() } }
                SymairaFilterMenu(
                    "Scope", selection: $vm.scope,
                    options: MemoryScopeFilter.allCases.map { (value: $0, label: $0.label) }
                )
                .frame(width: 170)
                .onChange(of: vm.scope) { _, _ in Task { await vm.loadMemories() } }
                Button("Search") { Task { await vm.loadMemories() } }
                    .symairaButtonStyle(.secondary)
            }

            if vm.isLoading && vm.memories.isEmpty {
                SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Loading memories…", retry: refresh) { _ in
                    EmptyView()
                }
            } else if vm.memories.isEmpty {
                SymairaPageEmpty(
                    icon: .inbox,
                    title: vm.searchText.isEmpty ? "No Memories" : "No Matching Memories",
                    message: vm.searchText.isEmpty
                        ? "Nothing is stored for this scope yet. Use New Memory, or let an agent write through the memory server."
                        : "Nothing matches this search. Try fewer words or another scope."
                )
            } else {
                HStack(alignment: .top, spacing: SymairaSpacing.medium) {
                    memoryTable
                    memoryDetail
                        .frame(width: 300)
                }
                if let note = vm.listTruncationNote {
                    Text(note).symairaText(.caption)
                }
            }
        }
    }

    /// Keyed on `listGeneration`: a changed result set mounts a fresh table
    /// instead of diffing the mounted one, the path that re-entered AppKit's
    /// row-span cache (#231).
    private var memoryTable: some View {
        Table(vm.memories.sorted(using: sortOrder), selection: $vm.selectedMemoryID, sortOrder: $sortOrder) {
            TableColumn("Memory", value: \.content) { memory in
                Text(verbatim: memory.content).symairaText(.body).lineLimit(2)
            }
            .width(min: 180)
            TableColumn("Scope", value: \.scope) { memory in
                Text(verbatim: memory.scope).symairaText(.secondary)
            }
            .width(min: 56, ideal: 64)
            TableColumn("Tier", value: \.displayTier) { memory in
                Text(verbatim: memory.displayTier).symairaText(.secondary)
            }
            .width(min: 48, ideal: 56)
            TableColumn("Created", value: \.createdAt) { memory in
                Text(memory.formattedCreatedAt).symairaText(.monoSmall)
            }
            .width(min: 112, ideal: 120)
        }
        .id(vm.listGeneration)
        .contextMenu(forSelectionType: MemoryRecord.ID.self) { ids in
            if let memory = vm.memories.first(where: { ids.contains($0.id) }) {
                Button("Copy Content") { vm.copyToPasteboard(memory.content, label: "Memory") }
                Button("Copy ID") { vm.copyToPasteboard(memory.id, label: "Memory ID") }
                Divider()
                Button("Delete Memory…", role: .destructive) { memoryToDelete = memory }
            }
        }
        .onDeleteCommand {
            if let memory = vm.selectedMemory { memoryToDelete = memory }
        }
        .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
    }

    @ViewBuilder
    private var memoryDetail: some View {
        if let memory = vm.selectedMemory {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                    Text(verbatim: memory.content)
                        .symairaText(.body)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    SymairaFormDivider()
                    DetailRow(label: "ID", value: memory.id, monospaced: true)
                    DetailRow(label: "Scope", value: memory.scope)
                    DetailRow(label: "Tier", value: memory.displayTier)
                    DetailRow(label: "Created", value: memory.formattedCreatedAt)
                    if let author = memory.createdBy { DetailRow(label: "Author", value: author) }
                    if let review = memory.reviewStatus { DetailRow(label: "Review", value: review) }
                    if let consolidation = memory.consolidationStatus {
                        DetailRow(label: "Consolidation", value: consolidation)
                    }
                    if let count = memory.accessCount { DetailRow(label: "Recalled", value: "\(count) times") }
                    if let expiresAt = memory.expiresAt {
                        DetailRow(label: "Expires", value: formatModuleTimestamp(expiresAt))
                    }
                    if !memory.metadataPairs.isEmpty {
                        Text("METADATA").symairaText(.sectionLabel).padding(.top, SymairaSpacing.small)
                        ForEach(memory.metadataPairs, id: \.key) { pair in
                            DetailRow(label: pair.key, value: pair.value, monospaced: true)
                        }
                    }
                    HStack(spacing: SymairaSpacing.medium) {
                        Button("Copy Content") { vm.copyToPasteboard(memory.content, label: "Memory") }
                            .symairaButtonStyle(.secondary)
                        Button("Delete…", role: .destructive) { memoryToDelete = memory }
                            .symairaButtonStyle(.secondary)
                    }
                    .padding(.top, SymairaSpacing.small)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .symairaCard()
        } else {
            SymairaEmptyState(
                icon: .sidebar,
                title: "No Memory Selected",
                message: "Select a memory to see its scope, provenance and metadata."
            )
            .symairaCard()
        }
    }

    // MARK: - Rules

    @ViewBuilder
    var rulesSection: some View {
        if vm.rules.isEmpty {
            SymairaPageEmpty(
                icon: .list,
                title: "No Rules",
                message: "Procedural rules that agents store in the memory core appear here."
            )
        } else {
            Table(vm.rules) {
                TableColumn("Scope") { rule in Text(verbatim: rule.scope).symairaText(.secondary) }
                    .width(min: 70, ideal: 90)
                TableColumn("Rule") { rule in Text(verbatim: rule.content).symairaText(.body).lineLimit(3) }
                TableColumn("Created") { rule in Text(rule.formattedCreatedAt).symairaText(.monoSmall) }
                    .width(min: 130, ideal: 150)
            }
            .contextMenu(forSelectionType: MemoryRule.ID.self) { ids in
                if let rule = vm.rules.first(where: { ids.contains($0.id) }) {
                    Button("Copy Rule") { vm.copyToPasteboard(rule.content, label: "Rule") }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
        }
    }

    // MARK: - Query log

    @ViewBuilder
    var queriesSection: some View {
        if let log = vm.queryLog, log.totalQueries > 0 {
            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                FlowLayout {
                    SymairaBadge("\(log.totalQueries) queries", tone: .informative)
                    ForEach(log.toolCounts.prefix(4), id: \.name) { SymairaBadge("\($0.name) \($0.count)") }
                    ForEach(log.actorCounts.prefix(3), id: \.name) { SymairaBadge("\($0.name) \($0.count)") }
                }
                Table(log.entries) {
                    TableColumn("Time") { entry in Text(entry.formattedCreatedAt).symairaText(.monoSmall) }
                        .width(min: 130, ideal: 150)
                    TableColumn("Actor") { entry in Text(verbatim: entry.actor ?? "—").symairaText(.secondary) }
                        .width(min: 80, ideal: 110)
                    TableColumn("Tool") { entry in Text(verbatim: entry.tool).symairaText(.mono) }
                        .width(min: 110, ideal: 140)
                    TableColumn("Query") { entry in Text(verbatim: entry.queryText ?? "—").symairaText(.body) }
                    TableColumn("Duration") { entry in
                        Text(entry.durationMs.map { "\($0) ms" } ?? "—")
                            .symairaText(.monoSmall)
                            .frame(maxWidth: .infinity, alignment: .trailing)
                    }
                    .width(min: 70, ideal: 80)
                }
                .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
            }
        } else {
            SymairaPageEmpty(
                icon: .chart,
                title: "No Queries Logged",
                message: "Every retrieval through the memory server is logged here once an agent searches the store."
            )
        }
    }

    // MARK: - Health

    var healthSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack {
                Text("Checks the database, the embedding backend and the memory configuration.")
                    .symairaText(.secondary)
                Spacer()
                Button("Run Memory Doctor") { Task { await vm.runDoctor() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isLoading)
            }
            if vm.isLoading {
                SymairaLoadingState("Running memory checks…")
            } else if let report = vm.doctorReport {
                ScrollView { OutputBlock(text: report) }
            } else {
                SymairaPageEmpty(icon: .diagnose, title: "No Report Yet", message: "Run Memory Doctor to check the store.")
            }
        }
    }
}
