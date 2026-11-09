import SwiftUI
import SymairaTheme
import SymBrainCore

/// Overview: is the gateway healthy, and what needs attention first.
///
/// Screen card — Destination: Overview (1 of 7). Job: see gateway health;
/// primary action only when Brain is not initialized (Initialize).
/// Data: `symbrain version` + `symbrain doctor` (live, on open and ⌘R) and
/// the local audit log. States: running health checks, CLI not found,
/// doctor error with Retry. Density: dashboard. Platform: macOS workspace.
struct OverviewPage: View {
    let client: SymBrainClient
    @Binding var destination: Destination

    @EnvironmentObject private var status: AppStatus
    @Environment(\.symairaTopography) private var topography
    @StateObject var vm: DashboardViewModel
    @StateObject var activity: AuditViewModel

    @State var hasLoaded = false
    @State var isInitializing = false
    @State var initializeError: String?
    @State var folderError: String?
    @State var moduleSheet: ManagedModule?

    init(client: SymBrainClient, destination: Binding<Destination>) {
        self.client = client
        _destination = destination
        _vm = StateObject(wrappedValue: DashboardViewModel(client: client))
        _activity = StateObject(wrappedValue: AuditViewModel(client: client))
    }

    var body: some View {
        SymairaPage(title: "Overview", subtitle: subtitle, maxWidth: PageWidth.dashboard) {
            Spacer()
            Button(action: refresh) {
                Label("Refresh", symairaIcon: .refresh)
            }
            .symairaButtonStyle(.secondary)
            .disabled(vm.isLoading)
        } content: {
            if vm.isBinaryNotFound {
                cliMissing
            } else {
                SymairaPageState(state: pageState, loading: "Running health checks…", retry: refresh) { report in
                    reportContent(report)
                }
            }
        }
        .task { await load() }
        .pageRefresh(refresh)
        .pagePrimaryAction(needsInitialization ? PageAction("Initialize Symaira Brain", isEnabled: !isInitializing) {
            Task { await initialize() }
        } : nil)
        .onChange(of: topographyState, initial: true) { _, state in topography?.state = state }
        .sheet(item: $moduleSheet) { module in
            ModuleSheet(module: module, client: client) { moduleSheet = nil }
                .frame(width: 600, height: 620)
        }
    }

    private var subtitle: String {
        guard let version = vm.versionInfo else { return "Health of the agent-context gateway on this Mac" }
        let platform = [version.os, version.arch].compactMap { $0 }.joined(separator: "/")
        return platform.isEmpty ? "symbrain CLI \(version.version)" : "symbrain CLI \(version.version) on \(platform)"
    }

    private var pageState: SymairaPageLoadState<DoctorReport> {
        PageLoad.state(
            hasLoaded: hasLoaded,
            error: vm.errorMessage, value: vm.doctorReport
        ).compactMapLoaded()
    }

    var needsInitialization: Bool {
        vm.doctorReport.map { !$0.config.exists } ?? false
    }

    private var topographyState: SymairaTopoState {
        if vm.isLoading || isInitializing { return .thinking }
        if vm.errorMessage != nil || vm.isBinaryNotFound { return .problem }
        if needsInitialization { return .input }
        return .idle
    }

    // MARK: - Loading

    func refresh() {
        Task { await load() }
    }

    private func load() async {
        async let doctor: Void = vm.refresh()
        async let calls: Void = activity.refresh()
        async let cli: Void = status.refresh(client: client)
        _ = await (doctor, calls, cli)
        hasLoaded = true
    }

    func initialize() async {
        isInitializing = true
        initializeError = nil
        defer { isInitializing = false }
        do {
            _ = try await client.initialize()
            await vm.refresh()
        } catch {
            initializeError = formatError(error).message
        }
    }

    func createFolder(_ path: String) {
        folderError = nil
        do {
            try FileManager.default.createDirectory(
                at: URL(fileURLWithPath: path), withIntermediateDirectories: true)
            refresh()
        } catch {
            folderError = "Could not create \(path). Check the folder permissions and try again."
        }
    }

    // MARK: - CLI missing

    private var cliMissing: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            SymairaPageEmpty(
                icon: .productBrain,
                title: "Symaira Brain CLI Not Found",
                message: "Install the CLI with Homebrew, then check again. A custom path can be set in Settings."
            ) {
                VStack(spacing: SymairaSpacing.medium) {
                    CommandLineView("brew install danieljustus/tap/symbrain")
                        .frame(maxWidth: 420)
                    Button(action: refresh) {
                        Label("Check Again", symairaIcon: .refresh)
                    }
                    .symairaButtonStyle(.primary)
                }
            }
            SymairaNotice(title: "Searched Locations", message: client.binarySearchDiagnostic, tone: .neutral)
        }
    }
}

private extension SymairaPageLoadState {
    /// Unwraps an optional loaded value; a missing value keeps loading.
    func compactMapLoaded<Wrapped>() -> SymairaPageLoadState<Wrapped> where Value == Wrapped? {
        switch self {
        case .idle: .idle
        case .loading: .loading
        case .failed(let message): .failed(message)
        case .loaded(let value): value.map { .loaded($0) } ?? .loading
        }
    }
}
