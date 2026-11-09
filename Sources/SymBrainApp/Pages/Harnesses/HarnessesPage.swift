import SwiftUI
import SymairaTheme
import SymBrainCore

/// Harnesses: connect Symaira Brain to agent harnesses and sync them.
///
/// Screen card — Destination: Harnesses (2 of 7). Job: keep harnesses in
/// sync; primary action Sync Harnesses (dry run by default). Data:
/// `symbrain doctor` harness list, `symbrain profile list`, `symbrain sync`
/// on demand. States: loading harnesses, no harness detected, doctor error
/// with Retry, sync error inline. Density: rows in cards. Platform: macOS;
/// preference key `syncDryRun` unchanged.
struct HarnessesPage: View {
    let client: SymBrainClient

    @AppStorage(PreferenceKey.syncDryRun) private var dryRun = true
    @StateObject var vm: HarnessesViewModel
    @StateObject var sync: SyncViewModel

    @State private var hasLoaded = false
    @State var preview: HarnessPreview?
    @State var pendingInstall: PendingInstall?
    @State var pendingUninstall: HarnessStatus?
    @State private var confirmLiveSync = false

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: HarnessesViewModel(client: client))
        let syncModel = SyncViewModel(client: client)
        syncModel.dryRun = UserDefaults.standard.object(forKey: PreferenceKey.syncDryRun) as? Bool ?? true
        _sync = StateObject(wrappedValue: syncModel)
    }

    var body: some View {
        SymairaPage(
            title: "Harnesses",
            subtitle: "Connect Symaira Brain to your agent harnesses and push instructions and skills",
            maxWidth: PageWidth.dashboard
        ) {
            Toggle("Dry Run", isOn: $dryRun)
                .toggleStyle(.symairaSwitch)
                .help("Preview what a sync would write without writing it")
            Spacer()
            Button(action: refresh) { Label("Refresh", symairaIcon: .refresh) }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isLoading)
            Button(action: startSync) { Label("Sync Harnesses", symairaIcon: .sync) }
                .symairaButtonStyle(.primary)
                .disabled(sync.isLoading)
        } content: {
            SymairaPageState(state: pageState, loading: "Loading harnesses…", retry: refresh) { harnesses in
                loaded(harnesses)
            }
        }
        .task {
            await vm.refresh()
            hasLoaded = true
        }
        .pageRefresh(refresh)
        .pagePrimaryAction(PageAction("Sync Harnesses", isEnabled: !sync.isLoading, perform: startSync))
        .onChange(of: dryRun) { _, value in
            sync.dryRun = value
            // Entering dry run refreshes the preview; leaving it only clears
            // the stale preview and never writes (#148).
            Task { await sync.dryRunChanged(to: value) }
        }
        .sheet(item: $preview) { request in
            HarnessPreviewSheet(request: request, client: client, profiles: validProfiles) { preview = nil }
                .frame(width: 600, height: 520)
        }
        .alert(
            "Replace the \(pendingInstall?.harness.name ?? "") Configuration?",
            isPresented: Binding(get: { pendingInstall != nil }, set: { if !$0 { pendingInstall = nil } }),
            presenting: pendingInstall
        ) { request in
            Button("Install Profile", role: .destructive) {
                Task { await vm.install(harness: request.harness.name, profile: request.profile, dryRun: false) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { request in
            Text("Installing profile \(request.profile) rewrites \(request.harness.configPath).")
        }
        .alert(
            "Uninstall Symaira Brain from \(pendingUninstall?.name ?? "")?",
            isPresented: Binding(get: { pendingUninstall != nil }, set: { if !$0 { pendingUninstall = nil } }),
            presenting: pendingUninstall
        ) { harness in
            Button("Uninstall", role: .destructive) {
                Task { await vm.uninstall(harness: harness.name, dryRun: false) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { harness in
            Text("This removes the symbrain entry from \(harness.configPath).")
        }
        .alert("Run a Live Sync?", isPresented: $confirmLiveSync) {
            Button("Run Live Sync", role: .destructive) {
                sync.confirmLiveSync()
                Task { await sync.sync() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Dry Run is off, so this writes instruction files into \(sync.syncWorkingDirectory) and can overwrite existing harness configuration.")
        }
    }

    private var pageState: SymairaPageLoadState<[HarnessStatus]> {
        PageLoad.state(hasLoaded: hasLoaded, error: vm.errorMessage, value: vm.harnesses)
    }

    var validProfiles: [ProfileSummary] {
        vm.profiles.filter { $0.error == nil }
    }

    private func refresh() {
        Task { await vm.refresh() }
    }

    private func startSync() {
        if sync.canSyncImmediately {
            Task { await sync.sync() }
        } else {
            confirmLiveSync = true
        }
    }

    // MARK: - Loaded

    @ViewBuilder
    private func loaded(_ harnesses: [HarnessStatus]) -> some View {
        let installable = harnesses.filter(\.supportsMcpInstall)
        let instructionOnly = harnesses.filter { !$0.supportsMcpInstall }
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            if !dryRun {
                SymairaNotice(
                    title: "Live Sync Is On",
                    message: "Sync Harnesses writes instruction files directly. Dry Run stays off across launches until you turn it back on.",
                    tone: .warning
                )
            }
            if let result = vm.operationResult {
                SymairaNotice(message: result, tone: .positive) { vm.operationResult = nil }
            }
            if installable.isEmpty {
                SymairaPageEmpty(
                    icon: .terminal,
                    title: "No Harnesses Detected",
                    message: "Install a supported harness such as Claude Code, Codex or Cursor, then refresh."
                )
            } else {
                SymairaFormSection("MCP Harnesses", footer: "Each harness reaches the state cores through the profile it was installed with.") {
                    ForEach(Array(installable.enumerated()), id: \.element.name) { index, harness in
                        if index > 0 { SymairaFormDivider() }
                        harnessRow(harness)
                    }
                }
            }
            if !instructionOnly.isEmpty {
                SymairaFormSection("Skills and Instructions Only", footer: "These harnesses take no MCP entry; Sync Harnesses still updates them.") {
                    ForEach(Array(instructionOnly.enumerated()), id: \.element.name) { index, harness in
                        if index > 0 { SymairaFormDivider() }
                        SymairaFormRow(harness.name, description: harness.configPath)
                    }
                }
            }
            syncResults
        }
    }
}

/// A preview run captured at the moment of the tap (#83), so the sheet is
/// always built from this value rather than from stale view state.
struct HarnessPreview: Identifiable {
    let id = UUID()
    let harness: String
    let isInstall: Bool
}

/// An install over an existing config, waiting for confirmation (#75).
struct PendingInstall: Identifiable {
    let id = UUID()
    let harness: HarnessStatus
    let profile: String
}
