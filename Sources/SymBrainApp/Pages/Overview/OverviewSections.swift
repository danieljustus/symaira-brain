import SwiftUI
import SymairaTheme
import SymBrainCore

/// The loaded overview, ordered as LAYOUT-8 asks: attention first, then
/// summary tiles, then detail sections, then the event log.
extension OverviewPage {
    @ViewBuilder
    func reportContent(_ report: DoctorReport) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            attention(report)
            tiles(report)
            stateCores(report)
            harnesses(report)
            modules(report)
            configuration(report)
            recentActivity
        }
    }

    // MARK: - Attention

    @ViewBuilder
    private func attention(_ report: DoctorReport) -> some View {
        if !report.config.exists {
            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                SymairaNotice(
                    title: "Symaira Brain Is Not Initialized",
                    message: "Initialize to create the config, data and cache folders and the example profiles.",
                    tone: .warning
                )
                HStack(spacing: SymairaSpacing.medium) {
                    Button(action: { Task { await initialize() } }) {
                        Label(isInitializing ? "Initializing…" : "Initialize Symaira Brain", symairaIcon: .play)
                    }
                    .symairaButtonStyle(.primary)
                    .disabled(isInitializing)
                    if let initializeError {
                        Text(initializeError)
                            .symairaText(.caption)
                            .foregroundStyle(SymairaTokens.critical)
                    }
                }
            }
        } else if !report.config.parsed {
            SymairaNotice(
                title: "Config Could Not Be Read",
                message: report.config.error ?? "Fix the syntax in \(report.config.path), then refresh.",
                tone: .critical
            )
        }
        if let folderError {
            SymairaNotice(message: folderError, tone: .critical) { self.folderError = nil }
        }
    }

    // MARK: - Tiles

    private func tiles(_ report: DoctorReport) -> some View {
        let installable = report.harnesses.filter(\.supportsMcpInstall)
        let folders = [report.configDir, report.dataDir, report.cacheDir]
        return LazyVGrid(
            columns: [GridItem(.adaptive(minimum: 180), spacing: SymairaSpacing.medium)],
            spacing: SymairaSpacing.medium
        ) {
            StatTile(value: "\(report.profiles.count)", label: "Profiles")
            StatTile(
                value: "\(installable.filter(\.installed).count)/\(installable.count)",
                label: "Harnesses Connected"
            )
            StatTile(
                value: "\(report.servers.filter(\.found).count)/\(report.servers.count)",
                label: "External Services"
            )
            StatTile(value: "\(folders.filter(\.exists).count)/\(folders.count)", label: "Folders Ready")
        }
    }

    // MARK: - Sections

    private func stateCores(_ report: DoctorReport) -> some View {
        SymairaFormSection("External Services", footer: "Memory and skills run inside the symbrain CLI; the vault stays a separate process.") {
            ForEach(Array(report.servers.enumerated()), id: \.element.name) { index, server in
                if index > 0 { SymairaFormDivider() }
                SymairaFormRow(server.name.capitalized, description: server.found ? server.path : server.installHint) {
                    SymairaStatusLabel(
                        server.found ? (server.version.map { "Version \($0)" } ?? "Available") : "Not Found",
                        tone: server.found ? .positive : .critical
                    )
                }
            }
        }
    }

    private func harnesses(_ report: DoctorReport) -> some View {
        SymairaFormSection("Harnesses", footer: "Install Symaira Brain into a harness or change its profile on the Harnesses page.") {
            ForEach(Array(report.harnesses.enumerated()), id: \.element.name) { index, harness in
                if index > 0 { SymairaFormDivider() }
                SymairaFormRow(harness.name, description: harnessDescription(harness)) {
                    SymairaStatusLabel(harnessStatus(harness), tone: harnessTone(harness))
                }
            }
        }
    }

    private func modules(_ report: DoctorReport) -> some View {
        SymairaFormSection(
            "Optional Modules",
            footer: "A module stays hidden from every harness until a profile names it."
        ) {
            ForEach(Array(ManagedModule.allCases.enumerated()), id: \.element) { index, module in
                if index > 0 { SymairaFormDivider() }
                let installed = ManagedModuleSupport.installedModule(module: module.rawValue, report: report)
                SymairaFormRow(module.title, description: module.summary) {
                    HStack(spacing: SymairaSpacing.medium) {
                        SymairaStatusLabel(
                            installed.map { "Installed \($0.provenance?.version ?? "")" } ?? "Not Installed",
                            tone: installed == nil ? .neutral : .positive
                        )
                        Button("Details…") { moduleSheet = module }
                            .symairaButtonStyle(.secondary)
                            .accessibilityLabel("\(module.title) details")
                    }
                }
                .contextMenu {
                    Button("Show \(module.title) Details…") { moduleSheet = module }
                }
            }
        }
    }

    private func configuration(_ report: DoctorReport) -> some View {
        SymairaFormSection("Configuration") {
            SymairaFormRow("Config File", description: report.config.path) {
                SymairaStatusLabel(configStatus(report.config), tone: configTone(report.config))
            }
            folderRow("Config Folder", report.configDir)
            folderRow("Data Folder", report.dataDir)
            folderRow("Cache Folder", report.cacheDir)
        }
    }

    @ViewBuilder
    private func folderRow(_ title: String, _ folder: DirStatus) -> some View {
        SymairaFormDivider()
        SymairaFormRow(title, description: folder.path) {
            if folder.exists {
                SymairaStatusLabel("Ready", tone: .positive)
            } else {
                HStack(spacing: SymairaSpacing.medium) {
                    SymairaStatusLabel("Missing", tone: .warning)
                    Button("Create Folder") { createFolder(folder.path) }
                        .symairaButtonStyle(.secondary)
                        .accessibilityLabel("Create \(title)")
                }
            }
        }
        .contextMenu {
            Button("Copy Path") { Pasteboard.copy(folder.path) }
        }
    }

    private var recentActivity: some View {
        CardSection("Recent Activity") {
            Button("Show All Activity") { destination = .activity }
                .symairaButtonStyle(.secondary)
        } content: {
            if activity.entries.isEmpty {
                Text("No tool calls logged yet. Calls appear once a harness routes one through Symaira Brain.")
                    .symairaText(.caption)
            } else {
                ForEach(Array(activity.entries.prefix(6).enumerated()), id: \.element.id) { index, entry in
                    if index > 0 { SymairaFormDivider() }
                    HStack(spacing: SymairaSpacing.medium) {
                        Text(entry.formattedTime)
                            .symairaText(.monoSmall)
                            .frame(width: 150, alignment: .leading)
                        Text(verbatim: entry.tool)
                            .symairaText(.body)
                            .lineLimit(1)
                        Text(verbatim: entry.server)
                            .symairaText(.caption)
                        Spacer()
                        SymairaStatusLabel(entry.status, tone: StatusTone.call(entry.status))
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    // MARK: - Wording

    private func harnessDescription(_ harness: HarnessStatus) -> String {
        guard harness.supportsMcpInstall else { return "Receives skills and instructions only" }
        if let profile = harness.profile { return "Profile \(profile)" }
        return harness.configPath
    }

    private func harnessStatus(_ harness: HarnessStatus) -> String {
        if !harness.supportsMcpInstall { return "No MCP Config" }
        if harness.configFound && !harness.configParsed { return "Config Unreadable" }
        if harness.installed { return harness.profileMissing ? "Profile Missing" : "Connected" }
        return "Not Connected"
    }

    private func harnessTone(_ harness: HarnessStatus) -> SymairaTone {
        if !harness.supportsMcpInstall { return .informative }
        if harness.configFound && !harness.configParsed { return .critical }
        if harness.installed { return harness.profileMissing ? .warning : .positive }
        return .neutral
    }

    private func configStatus(_ config: ConfigStatus) -> String {
        if !config.exists { return "Missing" }
        return config.parsed ? "Valid" : "Unreadable"
    }

    private func configTone(_ config: ConfigStatus) -> SymairaTone {
        if !config.exists { return .warning }
        return config.parsed ? .positive : .critical
    }
}
