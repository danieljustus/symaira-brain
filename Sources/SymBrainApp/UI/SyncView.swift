import SwiftUI
import SymairaTheme
import SymBrainCore

struct SyncView: View {
    let client: SymBrainClient

    /// Persist the Dry Run toggle symmetrically across launches (#310).
    /// Both ON (safe default) and OFF states survive app relaunch deterministically.
    @AppStorage("syncDryRun") private var dryRun = true

    @StateObject private var vm: SyncViewModel
    /// Confirmation for the first live (writing) sync of a session (#148).
    @State private var showLiveSyncConfirmation = false

    init(client: SymBrainClient) {
        self.client = client
        let initialDryRun = UserDefaults.standard.object(forKey: "syncDryRun") as? Bool ?? true
        let viewModel = SyncViewModel(client: client)
        viewModel.dryRun = initialDryRun
        _vm = StateObject(wrappedValue: viewModel)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                headerSection

                controlsSection

                if vm.isLoading {
                    SymairaLoadingState("Syncing harnesses...")
                } else if let error = vm.errorMessage {
                    SymairaNotice(title: "Error", message: error, tone: .critical)
                    if let detail = vm.errorDetail {
                        SymairaNotice(title: nil, message: detail, tone: .informative)
                    }
                } else if let summary = vm.syncSummary {
                    targetsSection(summary)
                    skillsSection(summary)
                } else {
                    SymairaEmptyState(
                        systemImage: "arrow.triangle.2.circlepath",
                        title: "Sync Ready",
                        message: "Run sync to propagate profile and skill changes to your installed harnesses. Instruction files are written relative to the working directory: CLAUDE.md for Claude, .cursor/rules/symbrain.mdc for Cursor, GEMINI.md for Gemini. Harnesses without an instruction adapter (e.g., Codex) are skipped. Enable Dry Run to preview without writing."
                    )
                }
            }
            .padding(SymairaSpacing.xLarge)
        }
        .onAppear {
            vm.dryRun = dryRun
        }
        // #148: the first live sync of a session asks for confirmation
        // before writing, since it can overwrite harness configuration.
        .alert("Run Live Sync?", isPresented: $showLiveSyncConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Run Sync", role: .destructive) {
                vm.confirmLiveSync()
                Task { await vm.sync() }
            }
        } message: {
            Text("Dry Run is off. This writes instruction files into \(vm.syncWorkingDirectory) and can overwrite existing harness configuration.")
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text("Sync")
                .font(.title.bold())
                .foregroundStyle(SymairaTheme.textPrimary)
            Text("Push instructions and skills to installed harnesses")
                .font(.subheadline)
                .foregroundStyle(SymairaTheme.textSecondary)
        }
    }

    // MARK: - Controls

    private var controlsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                Toggle(isOn: $dryRun) {
                    Text("Dry Run")
                        .foregroundStyle(SymairaTheme.textSecondary)
                }
                .toggleStyle(.switch)
                .onChange(of: dryRun) { _, newValue in
                    vm.dryRun = newValue
                    // #148: entering dry-run refreshes the preview (safe);
                    // leaving dry-run only clears the stale preview, never syncs.
                    Task { await vm.dryRunChanged(to: newValue) }
                }

                Spacer()

                // #148: a live (writing) sync runs only from Sync Now, and the
                // first one of a session is gated behind a confirmation alert.
                Button(action: {
                    if vm.canSyncImmediately {
                        Task { await vm.sync() }
                    } else {
                        showLiveSyncConfirmation = true
                    }
                }) {
                    HStack(spacing: SymairaSpacing.medium) {
                        if vm.isLoading {
                            ProgressView()
                                .scaleEffect(0.7)
                                .frame(width: 14, height: 14)
                        } else {
                            Image(systemName: "arrow.triangle.2.circlepath")
                        }
                        Text("Sync Now")
                    }
                }
                .symairaButtonStyle(.primary)
                .accessibilityLabel(vm.isLoading ? "Syncing" : "Sync Now")
                .disabled(vm.isLoading)
            }

            // Warning copy aligned with persisted-off behavior (#310):
            // Informs the user that dry run stays disabled across relaunches until turned back on.
            if !dryRun {
                SymairaNotice(
                    title: "Live Mode Armed",
                    message: "Dry Run is disabled and stays off across app relaunches until enabled again. Live sync writes instruction files directly.",
                    tone: .warning
                )
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }

    // MARK: - Targets

    private func targetsSection(_ summary: SyncSummary) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("Targets")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)

            if summary.targets.isEmpty {
                Text("No harness targets found.")
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textMuted)
            } else {
                // #147: relative targets are resolved against this directory.
                Text("Paths resolve against \(vm.syncWorkingDirectory)")
                    .font(.caption2)
                    .foregroundStyle(SymairaTheme.textMuted)
                VStack(spacing: SymairaSpacing.xSmall) {
                    ForEach(summary.targets) { target in
                        targetRow(target)
                    }
                }
            }
        }
        .padding(SymairaSpacing.xLarge)
        .glassCard()
    }

    private func targetRow(_ target: SyncTargetStatus) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: SymairaSpacing.small) {
                    Text(target.name)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(SymairaTheme.textPrimary)
                    SymairaBadge(target.status, tone: statusTone(target.status))
                }
                Text(vm.displayPath(for: target))
                    .font(.caption2)
                    .foregroundStyle(SymairaTheme.textMuted)
                    .lineLimit(1)
                if let message = target.message, !message.isEmpty {
                    Text(message)
                        .font(.caption2)
                        .foregroundStyle(SymairaTheme.textSecondary)
                        .lineLimit(2)
                }
            }
            Spacer()
        }
        .padding(.vertical, SymairaSpacing.xSmall)
    }

    // MARK: - Skills

    private func skillsSection(_ summary: SyncSummary) -> some View {
        Group {
            if !summary.skills.isEmpty {
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    Text("Skills")
                        .font(.headline)
                        .foregroundStyle(SymairaTheme.goldPrimary)

                    VStack(spacing: SymairaSpacing.xSmall) {
                        ForEach(summary.skills) { skill in
                            skillRow(skill)
                        }
                    }
                }
                .padding(SymairaSpacing.xLarge)
                .glassCard()
            }
        }
    }

    private func skillRow(_ skill: SyncSkillResult) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: SymairaSpacing.small) {
                    Text(skill.name)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(SymairaTheme.textPrimary)
                    SymairaBadge(skill.status, tone: statusTone(skill.status))
                }
                if let message = skill.message, !message.isEmpty {
                    Text(message)
                        .font(.caption2)
                        .foregroundStyle(SymairaTheme.textSecondary)
                        .lineLimit(2)
                }
            }
            Spacer()
            if let durationMs = skill.durationMs {
                Text(formattedDuration(durationMs))
                    .font(.caption2.monospaced())
                    .foregroundStyle(SymairaTheme.textMuted)
            }
        }
        .padding(.vertical, SymairaSpacing.xSmall)
    }

    // MARK: - Helpers

    private func statusTone(_ status: String) -> SymairaTone {
        switch status.lowercased() {
        case "created", "updated": return .positive
        case "unchanged": return .neutral
        case "skipped": return .warning
        case "error": return .critical
        default: return .neutral
        }
    }

    private func formattedDuration(_ ms: Int64) -> String {
        if ms >= 1000 {
            let seconds = Double(ms) / 1000.0
            return String(format: "%.1fs", seconds)
        }
        return "\(ms)ms"
    }
}
