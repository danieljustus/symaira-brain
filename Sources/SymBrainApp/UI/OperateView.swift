import AppKit
import SwiftUI
import SymairaTheme
import SymBrainCore

/// The embedded Symaira Operate module: screen automation and permission
/// status. This screen only answers "is this Mac set up for it, and what
/// does symoperate itself report" — it does not drive automation, which
/// stays in the CLI and the MCP server.
struct OperateView: View {
    @StateObject private var vm: OperateViewModel
    @State private var tab: Tab = .status

    enum Tab: String, CaseIterable, Identifiable {
        case status = "Status"
        case activity = "Broker Activity"

        var id: String { rawValue }
    }

    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: OperateViewModel(symbrainClient: client))
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
                    SymairaLoadingState("Checking the Operate module\u{2026}")
                case .missing:
                    missingBinarySection
                case .failed(let message):
                    SymairaNotice(title: "Operate unavailable", message: message, tone: .critical)
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
                Text("Operate")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Symaira Operate — screen automation and permission status")
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
            systemImage: "hand.tap",
            title: "Operate is disabled",
            message: "This module is off in symbrain's global config, so Brain does not manage a symoperate binary for it. Enabling it does not by itself expose it to any profile."
        ) {
            copyableCommand(vm.enableCommand)
        }
    }

    // MARK: - Missing binary

    private var missingBinarySection: some View {
        SymairaEmptyState(
            systemImage: "hand.tap",
            title: "symoperate not installed",
            message: "No managed symoperate binary was found. Build it from the in-repo receiving copy, or check again if you installed it another way."
        ) {
            VStack(spacing: SymairaSpacing.medium) {
                copyableCommand(vm.setupCommand)

                if vm.isBuilding {
                    SymairaLoadingState("Building symoperate from source\u{2026} this can take a few minutes.")
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
            doctorSection
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
                    Label("Check for Updates", systemImage: "network")
                }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isCheckingVersion)
            }
            Text("Contacts GitHub to check for a newer symoperate release.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textMuted)

            if vm.isCheckingVersion {
                SymairaLoadingState("Checking\u{2026}")
            } else if let version = vm.versionReport {
                Grid(alignment: .leading, horizontalSpacing: SymairaSpacing.large, verticalSpacing: SymairaSpacing.xSmall) {
                    GridRow {
                        Text("Installed").foregroundStyle(SymairaTheme.textSecondary)
                        Text(version.version)
                    }
                    if version.updateAvailable, let latest = version.latestVersion {
                        GridRow {
                            Text("Latest").foregroundStyle(SymairaTheme.textSecondary)
                            Text(latest)
                        }
                    }
                }
                .font(.caption)
            }
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private var doctorSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text("Health").font(.headline)
                Spacer()
                Button(action: { Task { await vm.runDoctor() } }) {
                    Label("Run Doctor", systemImage: "stethoscope")
                }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isRunningDoctor)
            }
            Text("Takes a real screenshot and reads the frontmost app's UI tree to prove those permissions actually work.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textMuted)

            if vm.isRunningDoctor {
                SymairaLoadingState("Probing screen capture and accessibility\u{2026}")
            } else if let doctor = vm.doctorReport {
                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                    HStack {
                        SymairaBadge(doctor.ok ? "ok" : "attention needed", tone: doctor.ok ? .positive : .warning)
                        Text("\(doctor.environment.appsCount) apps · \(doctor.environment.displaysCount) displays · macOS \(doctor.environment.macOSVersion)")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textSecondary)
                    }
                    HStack {
                        permissionBadge("Accessibility", granted: doctor.permissions.accessibilityGranted)
                        permissionBadge("Screen Recording", granted: doctor.permissions.screenRecordingGranted)
                    }
                    ForEach(doctor.recommendations, id: \.self) { note in
                        Text(note).font(.caption).foregroundStyle(SymairaTheme.textSecondary)
                    }
                }
            }
        }
        .padding(SymairaSpacing.large)
        .glassCard()
    }

    private func permissionBadge(_ name: String, granted: Bool) -> some View {
        HStack(spacing: 4) {
            Text(name).font(.caption)
            SymairaBadge(granted ? "granted" : "missing", tone: granted ? .positive : .critical)
        }
    }

    // MARK: - Broker activity

    private var activitySection: some View {
        ModuleActivityTable(
            entries: vm.brokerActivity,
            emptyMessage: "Calls routed to the operate server through symbrain appear here.",
            onRefresh: { Task { await vm.loadActivity() } }
        )
    }
}
