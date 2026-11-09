import SwiftUI
import SymairaTheme
import SymBrainCore

/// Memory: the facts and rules agents keep across sessions.
///
/// Screen card — Destination: Memory (4 of 7). Job: find, inspect and add
/// memories; primary action New Memory. Data: embedded memory store through
/// `symbrain memory` (list/search, rules, query log, doctor), loaded on open
/// and ⌘R. States: opening the store, CLI not found, store error with Retry,
/// no memories / no match. Density: sortable table with a detail column.
/// Platform: macOS workspace. Broker calls moved to Activity.
struct MemoryPage: View {
    enum Section: String, CaseIterable, Hashable {
        case memories, rules, queries, health

        var title: String {
            switch self {
            case .memories: "Memories"
            case .rules: "Rules"
            case .queries: "Queries"
            case .health: "Health"
            }
        }
    }

    @StateObject var vm: MemoryViewModel
    @State var section: Section = .memories
    @State private var showNewMemory = false
    @State var memoryToDelete: MemoryRecord?
    @State var sortOrder = [KeyPathComparator(\MemoryRecord.createdAt, order: .reverse)]

    /// Memory runs inside the same binary as the rest of the app, so a CLI
    /// path set in Settings reaches this page too.
    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: MemoryViewModel(client: MemoryClient(brain: client)))
    }

    var body: some View {
        SymairaPage(
            title: "Memory",
            subtitle: "Facts and rules your agents keep across sessions",
            scrolls: false,
            maxWidth: PageWidth.table
        ) {
            SymairaSegmentedControl(
                "Memory section", selection: $section,
                options: Section.allCases.map { (value: $0, label: $0.title) }
            )
            Spacer()
            SymairaIconButton("Refresh", icon: .refresh, action: refresh)
                .disabled(vm.isLoading)
            Button(action: { showNewMemory = true }) { Label("New Memory", symairaIcon: .add) }
                .symairaButtonStyle(.primary)
                .disabled(vm.availability != .ready)
        } content: {
            content
        }
        .task { await vm.refresh() }
        .pageRefresh(refresh)
        .pagePrimaryAction(PageAction("New Memory", isEnabled: vm.availability == .ready) { showNewMemory = true })
        .sheet(isPresented: $showNewMemory) {
            NewMemorySheet(vm: vm) { showNewMemory = false }
                .frame(width: 560, height: 520)
        }
        .confirmationDialog(
            "Delete This Memory?",
            isPresented: Binding(get: { memoryToDelete != nil }, set: { if !$0 { memoryToDelete = nil } }),
            presenting: memoryToDelete
        ) { memory in
            Button("Delete Memory", role: .destructive) {
                Task { _ = await vm.deleteMemory(id: memory.id) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("It is removed from the local store, and no harness can recall it afterwards.")
        }
    }

    func refresh() {
        Task { await vm.refresh() }
    }

    @ViewBuilder
    private var content: some View {
        switch vm.availability {
        case .checking:
            SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Opening the memory store…", retry: refresh) { _ in
                EmptyView()
            }
        case .missing:
            SymairaPageEmpty(
                icon: .productBrain,
                title: "Symaira Brain CLI Not Found",
                message: "Memory runs inside the symbrain CLI. Install it, then refresh this page."
            ) {
                CommandLineView(vm.homebrewCommand).frame(maxWidth: 420)
            }
        case .failed(let message):
            SymairaPageState(state: SymairaPageLoadState<Void>.failed(message), loading: "", retry: refresh) { _ in
                EmptyView()
            }
        case .ready:
            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                if let error = vm.errorMessage {
                    SymairaNotice(
                        message: [error, vm.errorDetail].compactMap { $0 }.joined(separator: "\n"),
                        tone: .critical
                    ) { vm.clearError() }
                }
                switch section {
                case .memories: memoriesSection
                case .rules: rulesSection
                case .queries: queriesSection
                case .health: healthSection
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)
        }
    }
}

/// Creates one memory in the local store.
struct NewMemorySheet: View {
    @ObservedObject var vm: MemoryViewModel
    let close: () -> Void

    @State private var content = ""
    @State private var scope = "global"
    @State private var kind = ""
    @State private var isSaving = false
    @State private var failed = false

    private static let scopes = ["global", "project", "agent", "user", "session"]
    private static let kinds = ["user", "feedback", "project", "reference"]

    private var isEmpty: Bool { content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        SymairaSheetScaffold(
            title: "New Memory",
            dismiss: close,
            primaryTitle: isSaving ? "Saving…" : "Save Memory",
            primaryDisabled: isEmpty || isSaving,
            primary: save
        ) {
            VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                Text("CONTENT").symairaText(.sectionLabel)
                TextEditor(text: $content)
                    .font(SymairaTypography.body)
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 140)
                    .padding(SymairaSpacing.small)
                    .background(SymairaTokens.raised, in: RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
                            .stroke(SymairaTokens.line, lineWidth: 1)
                    )
                    .accessibilityLabel("Memory content")
            }
            SymairaFormSection(footer: "Global memories reach every harness; narrower scopes stay with one project, agent, user or session.") {
                SymairaFormRow("Scope") {
                    SymairaFilterMenu("Scope", selection: $scope, options: Self.scopes.map { (value: $0, label: $0.capitalized) })
                        .frame(width: 180)
                }
                SymairaFormDivider()
                SymairaFormRow("Kind") {
                    SymairaFilterMenu(
                        "Kind", selection: $kind,
                        options: [(value: "", label: "None")] + Self.kinds.map { (value: $0, label: $0.capitalized) }
                    )
                    .frame(width: 180)
                }
            }
            if failed, let error = vm.errorMessage {
                SymairaNotice(title: "Memory Not Saved", message: error, tone: .critical)
            }
        }
    }

    private func save() {
        guard !isEmpty, !isSaving else { return }
        isSaving = true
        failed = false
        Task {
            let saved = await vm.addMemory(content: content, scope: scope, kind: kind.isEmpty ? nil : kind)
            isSaving = false
            if saved { close() } else { failed = true }
        }
    }
}
