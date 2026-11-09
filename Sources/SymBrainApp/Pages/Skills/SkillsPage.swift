import SwiftUI
import SymairaTheme
import SymBrainCore

/// Skills: the portable skill library and where it is installed.
///
/// Screen card — Destination: Skills (5 of 7). Job: see which skills exist
/// and whether every harness has the current version; primary action Sync
/// Skills on the Installs section. Data: embedded skills core through
/// `symbrain skills` (list, status, targets, log, doctor, sync), loaded on
/// open and ⌘R. States: connecting, CLI not found, error with Retry, empty
/// library / no match / no installs. Density: sortable tables with a detail
/// column. Platform: macOS workspace. Broker calls moved to Activity.
struct SkillsPage: View {
    enum Section: String, CaseIterable, Hashable {
        case library, installs, log, health

        var title: String {
            switch self {
            case .library: "Library"
            case .installs: "Installs"
            case .log: "Log"
            case .health: "Health"
            }
        }
    }

    @StateObject var vm: SkillsViewModel
    @State var section: Section = .library
    @State var confirmSync = false
    @State var installSort = [KeyPathComparator(\SkillInstallStatus.name)]
    @State var skillSort = [KeyPathComparator(\SkillSummary.name)]

    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: SkillsViewModel(client: SkillsClient(brain: client)))
    }

    var body: some View {
        SymairaPage(
            title: "Skills",
            subtitle: "Your portable skill library and where each skill is installed",
            scrolls: false,
            maxWidth: PageWidth.table
        ) {
            SymairaSegmentedControl(
                "Skills section", selection: $section,
                options: Section.allCases.map { (value: $0, label: $0.title) }
            )
            Spacer()
            SymairaIconButton("Refresh", icon: .refresh, action: refresh)
                .disabled(vm.isLoading)
        } content: {
            content
        }
        .task { await vm.refresh() }
        .pageRefresh(refresh)
        .pagePrimaryAction(section == .installs && vm.availability == .ready
            ? PageAction("Sync Skills", isEnabled: !vm.isSyncing) { confirmSync = true } : nil)
        .alert("Sync Skills to Every Harness?", isPresented: $confirmSync) {
            Button("Sync Skills") { Task { await vm.applySync() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Stale skills are rendered again and written into the harness skill folders on this Mac. Use Preview Sync to see the plan first.")
        }
    }

    func refresh() {
        Task { await vm.refresh() }
    }

    @ViewBuilder
    private var content: some View {
        switch vm.availability {
        case .checking:
            SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Connecting to the skills core…", retry: refresh) { _ in
                EmptyView()
            }
        case .missing:
            SymairaPageEmpty(
                icon: .library,
                title: "Symaira Brain CLI Not Found",
                message: "Skills run inside the symbrain CLI. Install it, then refresh this page."
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
                if let status = vm.statusMessage {
                    SymairaNotice(message: status, tone: .positive) { vm.statusMessage = nil }
                }
                switch section {
                case .library: librarySection
                case .installs: installsSection
                case .log: logSection
                case .health: healthSection
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)
        }
    }

    // MARK: - Library

    private var librarySection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search skills", text: $vm.searchText)
                    .textFieldStyle(.symaira)
                Text("\(vm.skills.count) of \(vm.library?.skills.count ?? 0)")
                    .symairaText(.monoSmall)
                    .accessibilityLabel("\(vm.skills.count) of \(vm.library?.skills.count ?? 0) skills shown")
            }
            ForEach(vm.library?.issueMessages ?? [], id: \.self) { issue in
                SymairaNotice(title: "Library Issue", message: issue, tone: .warning)
            }
            if vm.isLoading && vm.skills.isEmpty {
                SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Loading the skill library…", retry: refresh) { _ in
                    EmptyView()
                }
            } else if vm.skills.isEmpty {
                SymairaPageEmpty(
                    icon: .library,
                    title: vm.searchText.isEmpty ? "No Skills" : "No Matching Skills",
                    message: vm.searchText.isEmpty
                        ? "The library is empty. Import skills with symbrain skills import, then refresh."
                        : "Nothing matches this search. Try a shorter name."
                )
            } else {
                HStack(alignment: .top, spacing: SymairaSpacing.medium) {
                    skillTable
                    skillDetail.frame(width: 300)
                }
            }
        }
    }

    private var skillTable: some View {
        Table(vm.skills.sorted(using: skillSort), selection: $vm.selectedSkillName, sortOrder: $skillSort) {
            TableColumn("Skill", value: \.name) { skill in
                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                    Text(verbatim: skill.name).symairaText(.bodyEmphasized)
                    if let description = skill.description, !description.isEmpty {
                        Text(verbatim: description).symairaText(.caption).lineLimit(1)
                    }
                }
            }
            TableColumn("Harnesses") { skill in
                Text(skill.targets.joined(separator: ", ")).symairaText(.secondary).lineLimit(1)
            }
            .width(min: 120, ideal: 200)
        }
        .contextMenu(forSelectionType: SkillSummary.ID.self) { ids in
            if let skill = vm.skills.first(where: { ids.contains($0.id) }) {
                Button("Copy Name") { vm.copyToPasteboard(skill.name, label: "Skill name") }
                Button("Copy Path") { vm.copyToPasteboard(skill.path, label: "Skill path") }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
    }

    @ViewBuilder
    private var skillDetail: some View {
        if let skill = vm.selectedSkill {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                    Text(verbatim: skill.name).symairaText(.heading)
                    if let description = skill.description, !description.isEmpty {
                        Text(verbatim: description).symairaText(.secondary).textSelection(.enabled)
                    }
                    SymairaFormDivider()
                    DetailRow(label: "Path", value: skill.path, monospaced: true)
                    if let modified = skill.formattedModifiedAt { DetailRow(label: "Modified", value: modified) }
                    if let rendered = skill.lastRenderedAt { DetailRow(label: "Rendered", value: formatModuleTimestamp(rendered)) }
                    DetailRow(label: "Last Used", value: skill.formattedLastUsed)
                    if let source = skill.lastUsedSource { DetailRow(label: "Usage Source", value: source) }
                    if let installs = skill.installs, !installs.isEmpty {
                        Text("INSTALLS").symairaText(.sectionLabel).padding(.top, SymairaSpacing.small)
                        ForEach(installs, id: \.target) { install in
                            DetailRow(label: install.target, value: install.path ?? "—", monospaced: true)
                        }
                    }
                    Button("Copy Path") { vm.copyToPasteboard(skill.path, label: "Skill path") }
                        .symairaButtonStyle(.secondary)
                        .padding(.top, SymairaSpacing.small)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .symairaCard()
        } else {
            SymairaEmptyState(
                icon: .sidebar,
                title: "No Skill Selected",
                message: "Select a skill to see where it lives and which harnesses have it."
            )
            .symairaCard()
        }
    }
}
