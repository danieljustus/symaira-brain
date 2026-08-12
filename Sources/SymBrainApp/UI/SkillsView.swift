import SwiftUI
import SymairaTheme
import SymBrainCore

/// The embedded Symaira Skills module: browse the portable skill library,
/// see how each harness install stands, inspect the harness inventory, and
/// read both the symskills operation log and the symbrain broker audit trail
/// for the skills server.
struct SkillsView: View {
    @StateObject private var vm = SkillsViewModel()
    @State private var tab: Tab = .library
    @State private var showSyncConfirmation = false

    enum Tab: String, CaseIterable, Identifiable {
        case library = "Library"
        case installs = "Installs"
        case targets = "Harnesses"
        case activity = "Broker Activity"
        case log = "Log"
        case health = "Health"

        var id: String { rawValue }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            headerSection

            if vm.isBinaryNotFound {
                missingRuntimeSection
            } else {
                Picker("Section", selection: $tab) {
                    ForEach(Tab.allCases) { item in
                        Text(item.rawValue).tag(item)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                if let error = vm.errorMessage {
                    VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                        SymairaNotice(title: "Error", message: error, tone: .critical)
                        if let detail = vm.errorDetail {
                            SymairaNotice(title: "Details", message: detail, tone: .neutral)
                        }
                    }
                }

                switch tab {
                case .library: librarySection
                case .installs: installsSection
                case .targets: targetsSection
                case .activity: activitySection
                case .log: logSection
                case .health: healthSection
                }
            }
        }
        .padding(SymairaSpacing.xLarge)
        .task {
            await vm.refresh()
        }
        .alert("Sync Skills", isPresented: $showSyncConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Sync") { Task { await vm.applySync() } }
        } message: {
            Text(
                "This re-renders every stale skill and writes it into the harness skill "
                    + "directories on this Mac. Run a preview first if you want to see the plan."
            )
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Skills")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Symaira Skills — portable skill library and harness installs")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()

            if let version = vm.versionLine, !version.isEmpty {
                SymairaBadge(version, tone: .informative)
            }
            if let status = vm.statusMessage {
                SymairaBadge(status, tone: .positive)
            }

            Button(action: { Task { await vm.refresh() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.secondary)
        }
    }

    private var missingRuntimeSection: some View {
        SymairaEmptyState(
            systemImage: "square.stack.3d.up",
            title: "symskills not found",
            message: vm.errorMessage
                ?? "Install the Symaira Skills runtime, then reload this screen."
        ) {
            Button(action: { Task { await vm.refresh() } }) {
                Label("Check Again", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.primary)
        }
    }

    // MARK: - Library

    private var librarySection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search skills…", text: $vm.searchText)
                    .textFieldStyle(.roundedBorder)
                Text("\(vm.skills.count) of \(vm.library?.skills.count ?? 0)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(SymairaTheme.textMuted)
            }

            ForEach(vm.library?.issueMessages ?? [], id: \.self) { issue in
                SymairaNotice(title: "Library issue", message: issue, tone: .warning)
            }

            if vm.isLoading && vm.skills.isEmpty {
                SymairaLoadingState("Loading skill library…")
            } else if vm.skills.isEmpty {
                SymairaEmptyState(
                    systemImage: "square.stack.3d.up.slash",
                    title: "No Skills",
                    message: "The symskills library is empty, or nothing matches the search."
                )
            } else {
                HSplitView {
                    skillList
                    skillDetail
                }
            }
        }
    }

    private var skillList: some View {
        List(vm.skills, selection: $vm.selectedSkillName) { skill in
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text(skill.name)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(SymairaTheme.textPrimary)
                if let description = skill.description, !description.isEmpty {
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(SymairaTheme.textSecondary)
                        .lineLimit(2)
                }
                HStack(spacing: SymairaSpacing.small) {
                    ForEach(skill.targets, id: \.self) { target in
                        SymairaBadge(target, tone: .informative)
                    }
                }
            }
            .padding(.vertical, SymairaSpacing.xSmall)
            .contextMenu {
                Button("Copy Name") { vm.copyToPasteboard(skill.name, label: "Skill name") }
                Button("Copy Path") { vm.copyToPasteboard(skill.path, label: "Skill path") }
            }
        }
        .scrollContentBackground(.hidden)
        .frame(minWidth: 300, idealWidth: 380)
    }

    @ViewBuilder
    private var skillDetail: some View {
        if let skill = vm.selectedSkill {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    Text(skill.name)
                        .font(.title3.bold())
                        .foregroundStyle(SymairaTheme.textPrimary)

                    if let description = skill.description, !description.isEmpty {
                        Text(description)
                            .font(.callout)
                            .foregroundStyle(SymairaTheme.textSecondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }

                    detailRow("Path", skill.path)
                    if let modified = skill.formattedModifiedAt {
                        detailRow("Modified", modified)
                    }
                    if let rendered = skill.lastRenderedAt {
                        detailRow("Rendered", formatModuleTimestamp(rendered))
                    }
                    detailRow("Last used", skill.formattedLastUsed)
                    if let source = skill.lastUsedSource {
                        detailRow("Usage source", source)
                    }

                    if let installs = skill.installs, !installs.isEmpty {
                        Text("Installs")
                            .font(.headline)
                            .foregroundStyle(SymairaTheme.textPrimary)
                            .padding(.top, SymairaSpacing.small)
                        ForEach(installs, id: \.target) { install in
                            detailRow(install.target, install.path ?? "—")
                        }
                    }

                    Button(action: { vm.copyToPasteboard(skill.path, label: "Skill path") }) {
                        Label("Copy Path", systemImage: "doc.on.doc")
                    }
                    .symairaButtonStyle(.secondary)
                    .padding(.top, SymairaSpacing.small)
                }
                .padding(SymairaSpacing.large)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minWidth: 320)
        } else {
            SymairaEmptyState(
                systemImage: "sidebar.right",
                title: "No Skill Selected",
                message: "Select a skill to see where it lives and which harnesses have it."
            )
            .frame(minWidth: 320)
        }
    }

    // MARK: - Installs

    private var installsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                Picker("Harness", selection: $vm.targetFilter) {
                    ForEach(vm.targetOptions, id: \.self) { target in
                        Text(target == "all" ? "All Harnesses" : target).tag(target)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 200)
                .onChange(of: vm.targetFilter) { _, _ in
                    Task { await vm.loadStatus() }
                }

                Spacer()

                Button(action: { Task { await vm.previewSync() } }) {
                    Label("Preview Sync", systemImage: "eye")
                }
                .symairaButtonStyle(.secondary)

                Button(action: { showSyncConfirmation = true }) {
                    Label("Sync", systemImage: "arrow.triangle.2.circlepath")
                }
                .symairaButtonStyle(.primary)
            }

            // The counts sit on their own row: alongside the picker and the
            // two sync buttons they squeeze the buttons into ellipses.
            HStack(spacing: SymairaSpacing.small) {
                ForEach(vm.statusReport?.summary?.badges ?? [], id: \.label) { badge in
                    SymairaBadge("\(badge.count) \(badge.label)", tone: summaryTone(badge.label))
                }
                Spacer()
            }

            if vm.isSyncing {
                SymairaLoadingState("Running symskills sync…")
            } else if let plan = vm.syncPlan {
                syncPlanSection(
                    title: "Sync plan — nothing has been written",
                    countLabel: "planned",
                    report: plan,
                    tone: .informative
                )
            } else if let result = vm.syncResult {
                syncPlanSection(
                    title: "Sync result",
                    countLabel: "applied",
                    report: result,
                    tone: .positive
                )
            }

            if vm.statusReport?.rows.isEmpty ?? true {
                SymairaEmptyState(
                    systemImage: "checkmark.seal",
                    title: "No Managed Installs",
                    message: "Nothing is installed into a harness for this filter yet."
                )
            } else {
                installsTable
            }
        }
    }

    private var installsTable: some View {
        Table(vm.statusReport?.rows ?? []) {
            TableColumn("Harness") { row in
                SymairaBadge(row.target, tone: .informative)
            }
            .width(min: 90, ideal: 110)

            TableColumn("Skill") { row in
                Text(row.name)
                    .font(.caption.monospaced())
                    .foregroundStyle(SymairaTheme.textPrimary)
            }
            .width(min: 160, ideal: 220)

            TableColumn("Status") { row in
                SymairaBadge(row.status, tone: installTone(row.status))
            }
            .width(min: 80, ideal: 100)

            TableColumn("Mode") { row in
                Text(row.mode ?? "—")
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textMuted)
            }
            .width(min: 60, ideal: 80)

            TableColumn("Installed") { row in
                Text(row.formattedInstalledAt ?? "—")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(SymairaTheme.textMuted)
            }
            .width(min: 140, ideal: 160)
        }
    }

    private func syncPlanSection(
        title: String,
        countLabel: String,
        report: SkillSyncReport,
        tone: SymairaTone
    ) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                SymairaBadge("\(report.rows.count) \(countLabel)", tone: tone)
                Text(title)
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textSecondary)
                Spacer()
                Button("Dismiss") { vm.clearSyncPlan() }
                    .symairaButtonStyle(.secondary)
            }
            ForEach(report.rows.prefix(6)) { row in
                Text("\(row.action)  \(row.target)/\(row.name)")
                    .font(.caption.monospaced())
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            if report.rows.count > 6 {
                Text("… and \(report.rows.count - 6) more")
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textMuted)
            }
        }
    }

    // MARK: - Harness targets

    private var targetsSection: some View {
        Group {
            if vm.targetsReport?.rows.isEmpty ?? true {
                SymairaEmptyState(
                    systemImage: "desktopcomputer",
                    title: "No Harnesses Detected",
                    message: "symskills found no supported agent harness on this Mac."
                )
            } else {
                Table(vm.targetsReport?.rows ?? []) {
                    TableColumn("Harness") { target in
                        Text(target.title)
                            .font(.callout.weight(.medium))
                            .foregroundStyle(SymairaTheme.textPrimary)
                    }
                    .width(min: 120, ideal: 150)

                    TableColumn("State") { target in
                        SymairaBadge(
                            target.installState ?? (target.installed == true ? "present" : "absent"),
                            tone: target.installed == true ? .positive : .neutral
                        )
                    }
                    .width(min: 90, ideal: 110)

                    TableColumn("Skill Root") { target in
                        Text(target.effectiveSkillRoot ?? "—")
                            .font(.caption.monospaced())
                            .foregroundStyle(SymairaTheme.textSecondary)
                    }

                    TableColumn("Managed") { target in
                        Text("\(target.managedSkillsCount ?? 0)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textPrimary)
                    }
                    .width(min: 60, ideal: 70)

                    TableColumn("Unmanaged") { target in
                        Text("\(target.unmanagedSkillsCount ?? 0)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 70, ideal: 80)

                    TableColumn("Verified") { target in
                        SymairaBadge(
                            target.verificationStatus ?? "unknown",
                            tone: target.verificationStatus == "verified" ? .positive : .warning
                        )
                    }
                    .width(min: 80, ideal: 100)
                }
            }
        }
    }

    // MARK: - Broker activity

    private var activitySection: some View {
        ModuleActivityTable(
            entries: vm.brokerActivity,
            emptyMessage: "Calls routed to the skills server through symbrain appear here.",
            onRefresh: { vm.loadActivity() }
        )
    }

    // MARK: - symskills log

    private var logSection: some View {
        Group {
            if vm.logEntries.isEmpty {
                SymairaEmptyState(
                    systemImage: "list.bullet.rectangle.portrait",
                    title: "No Operations Logged",
                    message: "symskills records every render, install and sync it performs."
                )
            } else {
                Table(vm.logEntries) {
                    TableColumn("Time") { entry in
                        Text(entry.formattedTimestamp)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(SymairaTheme.textSecondary)
                    }
                    .width(min: 140, ideal: 160)

                    TableColumn("Event") { entry in
                        SymairaBadge(entry.event, tone: .informative)
                    }
                    .width(min: 80, ideal: 100)

                    TableColumn("Skill") { entry in
                        Text(entry.skill ?? "—")
                            .font(.caption.monospaced())
                            .foregroundStyle(SymairaTheme.textPrimary)
                    }
                    .width(min: 140, ideal: 180)

                    TableColumn("Harness") { entry in
                        Text(entry.target ?? "—")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 80, ideal: 100)

                    TableColumn("Outcome") { entry in
                        SymairaBadge(
                            entry.outcome ?? "—",
                            tone: entry.outcome == "ok" ? .positive : .critical
                        )
                    }
                    .width(min: 70, ideal: 90)

                    TableColumn("Actor") { entry in
                        Text(entry.actor ?? "—")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    }
                    .width(min: 70, ideal: 90)
                }
            }
        }
    }

    // MARK: - Health

    private var healthSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Button(action: { Task { await vm.runDoctor() } }) {
                Label("Run symskills doctor", systemImage: "stethoscope")
            }
            .symairaButtonStyle(.primary)

            if vm.isLoading {
                SymairaLoadingState("Running health checks…")
            } else if let report = vm.doctorReport {
                ScrollView {
                    VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                        if let versioning = report.versioningEnabled {
                            SymairaBadge(
                                versioning ? "versioning on" : "versioning off",
                                tone: versioning ? .positive : .warning
                            )
                        }
                        ForEach(report.pathRows, id: \.label) { row in
                            detailRow(row.label, row.value)
                        }

                        if let targets = report.targets, !targets.isEmpty {
                            Text("Target Roots")
                                .font(.headline)
                                .foregroundStyle(SymairaTheme.textPrimary)
                                .padding(.top, SymairaSpacing.small)
                            ForEach(targets) { target in
                                detailRow(target.target, target.user ?? target.project ?? "—")
                            }
                        }
                    }
                    .padding(SymairaSpacing.medium)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .background(
                    Color.white.opacity(0.04),
                    in: RoundedRectangle(cornerRadius: SymairaRadius.control)
                )
            } else {
                SymairaEmptyState(
                    systemImage: "stethoscope",
                    title: "No Report Yet",
                    message: "Run the doctor to see the library, render and target paths."
                )
            }
        }
    }

    // MARK: - Helpers

    private func detailRow(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top, spacing: SymairaSpacing.small) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textSecondary)
                .frame(width: 110, alignment: .leading)
            Text(value)
                .font(.caption.monospaced())
                .foregroundStyle(SymairaTheme.textPrimary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func installTone(_ status: String) -> SymairaTone {
        switch status {
        case "in-sync": .positive
        case "stale", "harness-changed": .warning
        case "conflict", "orphaned": .critical
        default: .neutral
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
}
