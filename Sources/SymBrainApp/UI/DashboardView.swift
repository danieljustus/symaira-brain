import SwiftUI
import SymairaTheme
import SymBrainCore

struct DashboardView: View {
    let client: SymBrainClient

    @StateObject private var vm: DashboardViewModel

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: DashboardViewModel(client: client))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                headerSection

                if vm.isLoading {
                    SymairaLoadingState("Running doctor...")
                } else if vm.isBinaryNotFound {
                    binaryNotFoundSection
                } else if let error = vm.errorMessage {
                    SymairaNotice(
                        title: "Error",
                        message: error,
                        tone: .critical
                    )
                } else if let report = vm.doctorReport {
                    doctorSection(report)
                }
            }
            .padding(SymairaSpacing.xLarge)
        }
        .task { await vm.refresh() }
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Image(systemName: "brain")
                    .font(.system(size: 28))
                    .foregroundStyle(SymairaTheme.goldPrimary)
                Text("SymBrain")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                if let version = vm.versionInfo {
                    SymairaBadge("v\(version.version)", tone: .positive)
                    if let os = version.os, let arch = version.arch {
                        SymairaBadge("\(os)/\(arch)", tone: .neutral)
                    }
                }
            }
            Text("Agent-context layer for the Symaira ecosystem")
                .font(.subheadline)
                .foregroundStyle(SymairaTheme.textSecondary)
        }
    }

    // MARK: - Binary Not Found (First-Run Help)

    private var binaryNotFoundSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            SymairaNotice(
                title: "SymBrain CLI Not Found",
                message: vm.errorMessage ?? "The symbrain CLI binary could not be found. Install it and try again.",
                tone: .critical
            )

            VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                Text("Install via Homebrew")
                    .font(.headline)
                    .foregroundStyle(SymairaTheme.goldPrimary)

                HStack(spacing: SymairaSpacing.small) {
                    Text("brew install danieljustus/tap/symbrain")
                        .font(.caption.monospaced())
                        .foregroundStyle(SymairaTheme.textPrimary)
                        .padding(SymairaSpacing.small)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))

                    Button(action: {
                        #if os(macOS)
                        let pasteboard = NSPasteboard.general
                        pasteboard.clearContents()
                        pasteboard.setString("brew install danieljustus/tap/symbrain", forType: .string)
                        #endif
                    }) {
                        Label("Copy", systemImage: "doc.on.doc")
                    }
                    .symairaButtonStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                    Text("Quick Start")
                        .font(.headline)
                        .foregroundStyle(SymairaTheme.goldPrimary)

                    installStep(1, "Install the CLI", "brew install danieljustus/tap/symbrain")
                    installStep(2, "Initialize", "symbrain init")
                    installStep(3, "Install per Harness", "symbrain install --harness claude --profile personal")
                    installStep(4, "Check Health", "symbrain doctor")
                }
            }
            .padding(SymairaSpacing.medium)
            .glassCard()

            HStack(spacing: SymairaSpacing.medium) {
                Button(action: { Task { await vm.refresh() } }) {
                    Label("Retry", systemImage: "arrow.clockwise")
                }
                .symairaButtonStyle(.primary)
            }

            Text("To configure a custom binary path, select Settings in the sidebar.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textSecondary)

            if let detail = vm.errorDetail {
                SymairaNotice(
                    title: "Details",
                    message: detail,
                    tone: .neutral
                )
            }
        }
    }

    private func installStep(_ number: Int, _ title: String, _ command: String) -> some View {
        HStack(spacing: SymairaSpacing.small) {
            Text("\(number)")
                .font(.caption.weight(.bold))
                .foregroundStyle(.black)
                .frame(width: 22, height: 22)
                .background(SymairaTheme.goldPrimary, in: Circle())
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textPrimary)
            Text(command)
                .font(.caption2.monospaced())
                .foregroundStyle(SymairaTheme.textSecondary)
        }
    }

    // MARK: - Doctor Report

    private func doctorSection(_ report: DoctorReport) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            // Directories
            Text("Directories")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            HStack(spacing: SymairaSpacing.medium) {
                dirCard("Config", status: report.configDir, purpose: "Stores profile configuration and settings")
                dirCard("Data", status: report.dataDir, purpose: "Stores persistent application data and audit logs")
                dirCard("Cache", status: report.cacheDir, purpose: "Stores temporary cached data")
            }

            // Config
            Text("Configuration")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            configCard(report.config)

            // Servers
            Text("State-Core Servers")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 260))], spacing: SymairaSpacing.medium) {
                ForEach(report.servers, id: \.name) { server in
                    serverCard(server)
                }
            }

            // Harnesses
            Text("Harnesses")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 260))], spacing: SymairaSpacing.medium) {
                ForEach(report.harnesses, id: \.name) { harness in
                    harnessCard(harness)
                }
            }

            // Profiles
            Text("Profiles (\(report.profiles.count))")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            HStack {
                ForEach(report.profiles, id: \.self) { name in
                    SymairaBadge(name, tone: .informative)
                }
            }
        }
    }

    // MARK: - Cards

    private func dirCard(_ title: String, status: DirStatus, purpose: String) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textSecondary)
            SymairaBadge(status.exists ? "Exists" : "Missing", tone: status.exists ? .positive : .warning)
            Text(status.path)
                .font(.caption2)
                .foregroundStyle(SymairaTheme.textMuted)
                .lineLimit(1)

            if !status.exists {
                Text(purpose)
                    .font(.caption2)
                    .foregroundStyle(SymairaTheme.textMuted)
                    .padding(.top, SymairaSpacing.xSmall)

                Button(action: { createDirectory(status.path) }) {
                    Label("Create Directory", systemImage: "folder.badge.plus")
                        .font(.caption)
                }
                .symairaButtonStyle(.secondary)
                .padding(.top, SymairaSpacing.xSmall)
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }

    private func createDirectory(_ path: String) {
        let url = URL(fileURLWithPath: path)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        Task { await vm.refresh() }
    }

    private func configCard(_ config: ConfigStatus) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                SymairaBadge(
                    config.exists ? "Found" : "Not Found",
                    tone: config.exists ? .positive : .neutral
                )
                // Only show parsed/parse-error badge when the file actually exists
                if config.exists {
                    SymairaBadge(
                        config.parsed ? "Parsed" : "Parse Error",
                        tone: config.parsed ? .positive : .critical
                    )
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: SymairaSpacing.xSmall) {
                Text(config.path)
                    .font(.caption2)
                    .foregroundStyle(SymairaTheme.textMuted)
                    .lineLimit(1)
                if !config.exists {
                    Text("File does not exist yet. Run symbrain init to create it.")
                        .font(.caption2)
                        .foregroundStyle(SymairaTheme.textMuted)
                } else if let error = config.error {
                    Text(error)
                        .font(.caption2)
                        .foregroundStyle(SymairaTheme.critical)
                        .lineLimit(2)
                }
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }

    private func serverCard(_ server: ServerStatus) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text(server.name.capitalized)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(SymairaTheme.textPrimary)
                Spacer()
                if server.found, let version = server.version {
                    SymairaBadge("v\(version)", tone: .positive)
                } else {
                    SymairaBadge("Not Found", tone: .critical)
                }
            }
            if let path = server.path {
                Text(path)
                    .font(.caption2)
                    .foregroundStyle(SymairaTheme.textMuted)
                    .lineLimit(1)
            }
            if let hint = server.installHint {
                Text(hint)
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.warning)
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }

    private func harnessCard(_ harness: HarnessStatus) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text(harness.name)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(SymairaTheme.textPrimary)
                Spacer()
                SymairaBadge(
                    harness.installed ? "Installed" : "Not Installed",
                    tone: harness.installed ? .positive : .neutral
                )
            }
            if let profile = harness.profile {
                Text("Profile: \(profile)")
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }
}
