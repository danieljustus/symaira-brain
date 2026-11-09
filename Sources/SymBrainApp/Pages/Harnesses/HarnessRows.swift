import SwiftUI
import SymairaTheme
import SymBrainCore

extension HarnessesPage {
    // MARK: - Harness row

    func harnessRow(_ harness: HarnessStatus) -> some View {
        SymairaFormRow(harness.name, description: rowDescription(harness)) {
            HStack(spacing: SymairaSpacing.medium) {
                SymairaStatusLabel(status(harness), tone: tone(harness))
                installMenu(harness)
                Menu {
                    actions(harness)
                } label: {
                    // Menu labels render only Image/Text; a live icon view stays blank.
                    Image(symairaIcon: .more, size: 18)
                        .foregroundStyle(SymairaTokens.ink2)
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("More Actions")
                .accessibilityLabel("More actions for \(harness.name)")
            }
        }
        .contextMenu { actions(harness) }
    }

    private func installMenu(_ harness: HarnessStatus) -> some View {
        Menu {
            if validProfiles.isEmpty {
                Text("Create a profile on the Profiles page first")
            }
            ForEach(validProfiles) { profile in
                Button(profile.name) { install(harness, profile: profile.name) }
            }
        } label: {
            Text(harness.installed ? "Change Profile" : "Install")
        }
        .menuStyle(.button)
        .symairaButtonStyle(.secondary)
        .fixedSize()
        .disabled(harness.configFound && !harness.configParsed)
        .help(harness.configFound && !harness.configParsed
            ? "Fix the unreadable config first; install refuses to rewrite it"
            : "Install Symaira Brain into \(harness.name) with a profile")
        .accessibilityLabel(harness.installed ? "Change profile for \(harness.name)" : "Install into \(harness.name)")
    }

    @ViewBuilder
    private func actions(_ harness: HarnessStatus) -> some View {
        Button("Preview Install…") { preview = HarnessPreview(harness: harness.name, isInstall: true) }
        if harness.installed {
            Button("Preview Uninstall…") { preview = HarnessPreview(harness: harness.name, isInstall: false) }
            Divider()
            Button("Uninstall…", role: .destructive) { pendingUninstall = harness }
        }
        Divider()
        Button("Copy Config Path") { Pasteboard.copy(harness.configPath) }
    }

    private func install(_ harness: HarnessStatus, profile: String) {
        if harness.installed {
            pendingInstall = PendingInstall(harness: harness, profile: profile)
        } else {
            Task { await vm.install(harness: harness.name, profile: profile, dryRun: false) }
        }
    }

    private func rowDescription(_ harness: HarnessStatus) -> String {
        if let profile = harness.profile, harness.installed {
            return "Profile \(profile) · \(harness.configPath)"
        }
        return harness.configPath
    }

    private func status(_ harness: HarnessStatus) -> String {
        if harness.configFound && !harness.configParsed { return "Config Unreadable" }
        if harness.installed { return harness.profileMissing ? "Profile Missing" : "Connected" }
        return harness.configFound ? "Not Connected" : "No Config Yet"
    }

    private func tone(_ harness: HarnessStatus) -> SymairaTone {
        if harness.configFound && !harness.configParsed { return .critical }
        if harness.installed { return harness.profileMissing ? .warning : .positive }
        return .neutral
    }

    // MARK: - Sync results

    @ViewBuilder
    var syncResults: some View {
        if sync.isLoading {
            SymairaLoadingState(sync.dryRun ? "Previewing sync…" : "Syncing harnesses…")
        } else if let error = sync.errorMessage {
            SymairaNotice(title: "Sync Failed", message: [error, sync.errorDetail].compactMap { $0 }.joined(separator: "\n"), tone: .critical)
        } else if let summary = sync.syncSummary {
            SymairaFormSection(
                sync.dryRun ? "Sync Preview" : "Last Sync",
                footer: sync.dryRun
                    ? "Nothing was written. Relative paths resolve against \(sync.syncWorkingDirectory)."
                    : "Relative paths resolve against \(sync.syncWorkingDirectory)."
            ) {
                if summary.targets.isEmpty {
                    SymairaFormRow("No Targets", description: "No harness takes instruction files from this sync.")
                }
                ForEach(Array(summary.targets.enumerated()), id: \.element.id) { index, target in
                    if index > 0 { SymairaFormDivider() }
                    SymairaFormRow(
                        target.name,
                        description: [sync.displayPath(for: target), target.message].compactMap { $0 }
                            .filter { !$0.isEmpty }.joined(separator: " · ")
                    ) {
                        SymairaStatusLabel(target.status.capitalized, tone: StatusTone.write(target.status))
                    }
                }
            }
            if !summary.skills.isEmpty {
                SymairaFormSection("Skills") {
                    ForEach(Array(summary.skills.enumerated()), id: \.element.id) { index, skill in
                        if index > 0 { SymairaFormDivider() }
                        SymairaFormRow(skill.name, description: skill.message) {
                            HStack(spacing: SymairaSpacing.medium) {
                                if let duration = skill.durationMs {
                                    Text(Self.duration(duration)).symairaText(.monoSmall)
                                }
                                SymairaStatusLabel(skill.status.capitalized, tone: StatusTone.write(skill.status))
                            }
                        }
                    }
                }
            }
        }
    }

    static func duration(_ ms: Int64) -> String {
        ms >= 1000 ? String(format: "%.1f s", Double(ms) / 1000) : "\(ms) ms"
    }
}
