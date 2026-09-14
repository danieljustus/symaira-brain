import AppKit
import SwiftUI
import SymairaTheme
import SymBrainCore

/// The embedded Symaira Scope module: local system inventory (ports, MCP
/// server configs, containers, daemons). This screen only answers "is this
/// module set up, and what does symscope report about itself" — the full
/// live inventory browser stays in the CLI (`symcockpit scope`), not here.
struct ScopeView: View {
    @StateObject private var vm: ScopeViewModel
    @State private var tab: Tab = .status

    enum Tab: String, CaseIterable, Identifiable {
        case status = "Status"
        case activity = "Broker Activity"

        var id: String { rawValue }
    }

    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: ScopeViewModel(symbrainClient: client))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            headerSection

            if let error = vm.errorMessage {
                SymairaNotice(title: "Error", message: error, tone: .critical)
            }

            if !vm.moduleEnabled {
                disabledSection
            } else {
                switch vm.availability {
                case .checking:
                    SymairaLoadingState("Checking the Scope module\u{2026}")
                case .missing:
                    missingBinarySection
                case .failed(let message):
                    SymairaNotice(title: "Scope unavailable", message: message, tone: .critical)
                case .ready:
                    ModuleTabStrip(selection: $tab)
                    switch tab {
                    case .status: statusSection
                    case .activity: activitySection
                    }
                }
            }
        }
        .padding(SymairaSpacing.xLarge)
        .task { await vm.refresh() }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Scope")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Symaira Scope — local system inventory")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()

            SymairaBadge(vm.moduleEnabled ? "module enabled" : "module disabled", tone: vm.moduleEnabled ? .positive : .neutral)
            SymairaBadge(availabilityLabel, tone: availabilityTone)
            if let version = vm.provenance?.version, !version.isEmpty {
                SymairaBadge(version, tone: .informative)
            }

            Button(action: { Task { await vm.refresh() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.secondary)
            .accessibilityLabel("Refresh")
        }
    }

    private var availabilityLabel: String {
        guard vm.moduleEnabled else { return "n/a" }
        switch vm.availability {
        case .checking: return "checking"
        case .missing: return "not installed"
        case .ready: return "installed"
        case .failed: return "error"
        }
    }

    private var availabilityTone: SymairaTone {
        guard vm.moduleEnabled else { return .neutral }
        switch vm.availability {
        case .ready: return .positive
        case .missing, .failed: return .critical
        case .checking: return .neutral
        }
    }

    // MARK: - Disabled in config

    private var disabledSection: some View {
        SymairaEmptyState(
            systemImage: "network",
            title: "Scope is disabled",
            message: "This module is off in symbrain's global config, so Brain does not manage a symscope binary for it. Enabling it does not by itself expose it to any profile."
        ) {
            copyableCommand(vm.enableCommand)
        }
    }

    // MARK: - Missing binary

    private var missingBinarySection: some View {
        SymairaEmptyState(
            systemImage: "network",
            title: "symscope not installed",
            message: "No managed symscope binary was found. Build it from the in-repo receiving copy, or check again if you installed it another way."
        ) {
            VStack(spacing: SymairaSpacing.medium) {
                copyableCommand(vm.setupCommand)

                if vm.isBuilding {
                    SymairaLoadingState("Building symscope from source\u{2026} this can take a few minutes.")
                } else {
                    if let sourceRoot = vm.sourceRoot {
                        selectedSourceRoot(sourceRoot)
                    } else {
                        Text("Choose a local Symaira Brain checkout before building. The selection is not saved.")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    }

                    Button(action: chooseSourceCheckout) {
                        Label(
                            vm.sourceRoot == nil ? "Choose Brain Source Checkout…" : "Change Brain Source Checkout…",
                            systemImage: "folder"
                        )
                    }
                    .symairaButtonStyle(.secondary)
                    .accessibilityLabel("Choose Brain Source Checkout")

                    if let sourceRoot = vm.sourceRoot {
                        Button(action: { Task { await vm.buildAndInstall(sourceRoot: sourceRoot) } }) {
                            Label("Build & Install Now", systemImage: "hammer")
                        }
                        .symairaButtonStyle(.primary)
                        .accessibilityLabel("Build and Install Now")
                    }
                }

                Button(action: { Task { await vm.refresh() } }) {
                    Label("Check Again", systemImage: "arrow.clockwise")
                }
                .symairaButtonStyle(.secondary)
                .accessibilityLabel("Check Again")
            }
        }
    }

    private func selectedSourceRoot(_ sourceRoot: URL) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text("Selected Brain checkout (not saved)")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textSecondary)
            Text(sourceRoot.path)
                .font(.caption.monospaced())
                .foregroundStyle(SymairaTheme.textPrimary)
                .textSelection(.enabled)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func chooseSourceCheckout() {
        let panel = NSOpenPanel()
        panel.title = "Choose Symaira Brain Source Checkout"
        panel.message = "Select the Brain checkout root containing the module source packages."
        panel.prompt = "Use Checkout"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false

        guard panel.runModal() == .OK, let sourceRoot = panel.url else { return }
        vm.selectSourceRoot(sourceRoot)
    }

    private func copyableCommand(_ command: String) -> some View {
        HStack(spacing: SymairaSpacing.small) {
            Text(command)
                .font(.callout.monospaced())
                .foregroundStyle(SymairaTheme.goldPrimary)
                .textSelection(.enabled)
            Button {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(command, forType: .string)
            } label: {
                Image(systemName: "doc.on.doc")
            }
            .buttonStyle(.plain)
            .help("Copy command")
        }
    }

    // MARK: - Status

    private var statusSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            binarySection
            profileSection
            versionSection
            scanSection
        }
    }

    private var binarySection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Managed binary").font(.headline)
            Grid(alignment: .leading, horizontalSpacing: SymairaSpacing.large, verticalSpacing: SymairaSpacing.xSmall) {
                GridRow {
                    Text("Path").foregroundStyle(SymairaTheme.textSecondary)
                    Text(vm.binaryPath ?? "—").font(.caption.monospaced()).foregroundStyle(SymairaTheme.textPrimary).textSelection(.enabled)
                }
                if let provenance = vm.provenance {
                    GridRow {
                        Text("Source").foregroundStyle(SymairaTheme.textSecondary)
                        SymairaBadge(provenance.source, tone: provenance.isBrainSource ? .informative : .neutral)
                    }
                    if let commit = provenance.receiverCommit {
                        GridRow {
                            Text("Receiver commit").foregroundStyle(SymairaTheme.textSecondary)
                            Text(String(commit.prefix(12))).font(.caption.monospaced()).textSelection(.enabled)
                        }
                    }
                    if let builder = provenance.builder {
                        GridRow {
                            Text("Builder").foregroundStyle(SymairaTheme.textSecondary)
                            Text(builder).font(.caption).textSelection(.enabled)
                        }
                    }
                    if let builtAt = provenance.formattedBuiltAt {
                        GridRow {
                            Text("Built").foregroundStyle(SymairaTheme.textSecondary)
                            Text(builtAt).font(.caption)
                        }
                    }
                    GridRow {
                        Text("SHA-256").foregroundStyle(SymairaTheme.textSecondary)
                        Text(String(provenance.binarySHA256.prefix(16)) + "\u{2026}").font(.caption.monospaced()).textSelection(.enabled)
                    }
                } else {
                    GridRow {
                        Text("Provenance").foregroundStyle(SymairaTheme.textSecondary)
                        Text("Unknown (no provenance sidecar next to this binary)").font(.caption).foregroundStyle(SymairaTheme.textMuted)
                    }
                }
            }
            .font(.caption)
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private var profileSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Profile exposure").font(.headline)
            if vm.profileExposure.isEmpty {
                Text("No profiles found.").font(.caption).foregroundStyle(SymairaTheme.textMuted)
            } else {
                ForEach(vm.profileExposure) { exposure in
                    HStack {
                        Text(exposure.profile).font(.caption.monospaced())
                        SymairaBadge(exposure.enabled ? "exposed" : "not exposed", tone: exposure.enabled ? .positive : .neutral)
                        if let mode = exposure.mode, !mode.isEmpty {
                            Text(mode).font(.caption).foregroundStyle(SymairaTheme.textMuted)
                        }
                    }
                }
            }
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private var versionSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text("Version").font(.headline)
                Spacer()
                Button(action: { Task { await vm.checkVersion() } }) {
                    Label("Check Version", systemImage: "info.circle")
                }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isCheckingVersion)
            }
            if vm.isCheckingVersion {
                SymairaLoadingState("Checking\u{2026}")
            } else if let version = vm.versionInfo {
                Grid(alignment: .leading, horizontalSpacing: SymairaSpacing.large, verticalSpacing: SymairaSpacing.xSmall) {
                    GridRow {
                        Text("symscope").foregroundStyle(SymairaTheme.textSecondary)
                        Text(version.version)
                    }
                    GridRow {
                        Text("Schema").foregroundStyle(SymairaTheme.textSecondary)
                        Text("\(version.schemaVersion)")
                    }
                }
                .font(.caption)
            }
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private var scanSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text("Health").font(.headline)
                Spacer()
                Button(action: { Task { await vm.runScan() } }) {
                    Label("Scan Now", systemImage: "magnifyingglass")
                }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isScanning)
            }
            Text("Walks listening ports, launchd/Homebrew daemons and Docker containers on this Mac — a real inventory pass, not a cheap check.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textMuted)

            if vm.isScanning {
                SymairaLoadingState("Scanning\u{2026}")
            } else if let snapshot = vm.snapshot {
                HStack(spacing: SymairaSpacing.large) {
                    countStat("Ports", snapshot.portCount)
                    countStat("MCP Servers", snapshot.mcpServerCount)
                    countStat("Containers", snapshot.containerCount)
                    countStat("Daemons", snapshot.daemonCount)
                }
                ForEach(snapshot.notes, id: \.self) { note in
                    Text(note).font(.caption).foregroundStyle(SymairaTheme.textSecondary)
                }
            }
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private func countStat(_ label: String, _ value: Int) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("\(value)").font(.title3.monospacedDigit()).foregroundStyle(SymairaTheme.textPrimary)
            Text(label).font(.caption).foregroundStyle(SymairaTheme.textMuted)
        }
    }

    // MARK: - Broker activity

    private var activitySection: some View {
        ModuleActivityTable(
            entries: vm.brokerActivity,
            emptyMessage: "Calls routed to the scope server through symbrain appear here.",
            onRefresh: { Task { await vm.loadActivity() } }
        )
    }
}
