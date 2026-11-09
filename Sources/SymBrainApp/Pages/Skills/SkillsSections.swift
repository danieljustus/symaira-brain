import SwiftUI
import SymairaTheme
import SymBrainCore

extension SkillsPage {
    // MARK: - Installs

    var installsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                SymairaFilterMenu(
                    "Harness", selection: $vm.targetFilter,
                    options: vm.targetOptions.map { (value: $0, label: $0 == "all" ? "All Harnesses" : $0) }
                )
                .frame(width: 190)
                .onChange(of: vm.targetFilter) { _, _ in Task { await vm.loadStatus() } }
                FlowLayout {
                    ForEach(vm.statusReport?.summary?.badges ?? [], id: \.label) { badge in
                        SymairaBadge("\(badge.count) \(badge.label)", tone: summaryTone(badge.label))
                    }
                }
                Spacer()
                Button("Preview Sync") { Task { await vm.previewSync() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isSyncing)
                Button(action: { confirmSync = true }) { Label("Sync Skills", symairaIcon: .sync) }
                    .symairaButtonStyle(.primary)
                    .disabled(vm.isSyncing)
            }

            harnessStrip

            if vm.isSyncing {
                SymairaLoadingState("Running skill sync…")
            } else if let plan = vm.syncPlan {
                syncReport(plan, title: "Sync Preview", footer: "Nothing has been written.")
            } else if let result = vm.syncResult {
                syncReport(result, title: "Sync Result", footer: nil)
            }

            if let rows = vm.statusReport?.rows, !rows.isEmpty {
                Table(rows.sorted(using: installSort), sortOrder: $installSort) {
                    TableColumn("Skill", value: \.name) { row in Text(verbatim: row.name).symairaText(.mono) }
                    TableColumn("Harness", value: \.target) { row in Text(verbatim: row.target).symairaText(.body) }
                        .width(min: 90, ideal: 120)
                    TableColumn("Status", value: \.status) { row in
                        SymairaStatusLabel(row.status, tone: StatusTone.write(row.status))
                    }
                    .width(min: 100, ideal: 130)
                    TableColumn("Mode") { row in Text(verbatim: row.mode ?? "—").symairaText(.secondary) }
                        .width(min: 60, ideal: 80)
                    TableColumn("Installed") { row in Text(row.formattedInstalledAt ?? "—").symairaText(.monoSmall) }
                        .width(min: 130, ideal: 150)
                }
                .contextMenu(forSelectionType: SkillInstallStatus.ID.self) { ids in
                    if let row = rows.first(where: { ids.contains($0.id) }) {
                        Button("Copy Skill Name") { vm.copyToPasteboard(row.name, label: "Skill name") }
                        Button("Show Only \(row.target)") { vm.targetFilter = row.target }
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
            } else {
                SymairaPageEmpty(
                    icon: .verified,
                    title: "No Managed Installs",
                    message: "No skill is installed into a harness for this filter yet. Sync Skills installs the library."
                )
            }
        }
    }

    /// One compact line per detected harness: skill root, counts, verification.
    @ViewBuilder
    private var harnessStrip: some View {
        if let targets = vm.targetsReport?.rows, !targets.isEmpty {
            ScrollView(.horizontal) {
                HStack(spacing: SymairaSpacing.medium) {
                    ForEach(targets) { target in
                        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                            Text(verbatim: target.title).symairaText(.bodyEmphasized)
                            Text("\(target.managedSkillsCount ?? 0) managed · \(target.unmanagedSkillsCount ?? 0) unmanaged")
                                .symairaText(.caption)
                            SymairaStatusLabel(
                                target.verificationStatus?.capitalized ?? "Unverified",
                                tone: target.verificationStatus == "verified" ? .positive : .warning
                            )
                        }
                        .symairaCard(padding: SymairaSpacing.medium)
                        .help(target.effectiveSkillRoot ?? "No skill folder detected")
                        .accessibilityElement(children: .combine)
                    }
                }
            }
            .scrollIndicators(.never)
        }
    }

    private func syncReport(_ report: SkillSyncReport, title: String, footer: String?) -> some View {
        SymairaFormSection(title, footer: footer) {
            HStack {
                Text("\(report.rows.count) changes").symairaText(.bodyEmphasized)
                Spacer()
                SymairaIconButton("Dismiss", icon: .close, size: 16) { vm.clearSyncPlan() }
            }
            ForEach(report.rows.prefix(6)) { row in
                Text("\(row.action)  \(row.target)/\(row.name)").symairaText(.monoSmall)
            }
            if report.rows.count > 6 {
                Text("\(report.rows.count - 6) more").symairaText(.caption)
            }
        }
    }

    private func summaryTone(_ label: String) -> SymairaTone {
        switch label {
        case "in sync": .positive
        case "stale", "harness changed", "unmanaged": .warning
        case "conflict", "orphaned": .critical
        default: .neutral
        }
    }

    // MARK: - Log

    @ViewBuilder
    var logSection: some View {
        if vm.logEntries.isEmpty {
            SymairaPageEmpty(
                icon: .list,
                title: "No Operations Logged",
                message: "Every render, install and sync the skills core performs is recorded here."
            )
        } else {
            Table(vm.logEntries) {
                TableColumn("Time") { entry in Text(entry.formattedTimestamp).symairaText(.monoSmall) }
                    .width(min: 130, ideal: 150)
                TableColumn("Event") { entry in Text(verbatim: entry.event).symairaText(.body) }
                    .width(min: 80, ideal: 100)
                TableColumn("Skill") { entry in Text(verbatim: entry.skill ?? "—").symairaText(.mono) }
                TableColumn("Harness") { entry in Text(verbatim: entry.target ?? "—").symairaText(.secondary) }
                    .width(min: 80, ideal: 100)
                TableColumn("Outcome") { entry in
                    SymairaStatusLabel(entry.outcome ?? "—", tone: entry.outcome == "ok" ? .positive : .critical)
                }
                .width(min: 80, ideal: 100)
                TableColumn("Actor") { entry in Text(verbatim: entry.actor ?? "—").symairaText(.secondary) }
                    .width(min: 70, ideal: 90)
            }
            .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
        }
    }

    // MARK: - Health

    var healthSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack {
                Text("Shows the library, render and harness folders the skills core uses.")
                    .symairaText(.secondary)
                Spacer()
                Button("Run Skills Doctor") { Task { await vm.runDoctor() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isLoading)
            }
            if vm.isLoading {
                SymairaLoadingState("Running skills checks…")
            } else if let report = vm.doctorReport {
                ScrollView {
                    VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                        if let versioning = report.versioningEnabled {
                            SymairaStatusLabel(
                                versioning ? "Versioning On" : "Versioning Off",
                                tone: versioning ? .positive : .warning
                            )
                        }
                        ForEach(report.pathRows, id: \.label) { row in
                            DetailRow(label: row.label, value: row.value, monospaced: true)
                        }
                        if let targets = report.targets, !targets.isEmpty {
                            Text("HARNESS FOLDERS").symairaText(.sectionLabel).padding(.top, SymairaSpacing.small)
                            ForEach(targets) { target in
                                DetailRow(label: target.target, value: target.user ?? target.project ?? "—", monospaced: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .symairaCard()
                }
            } else {
                SymairaPageEmpty(icon: .diagnose, title: "No Report Yet", message: "Run Skills Doctor to check the folders.")
            }
        }
    }
}
