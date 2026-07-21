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

    // MARK: - Doctor Report

    private func doctorSection(_ report: DoctorReport) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            // Directories
            Text("Directories")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)
            HStack(spacing: SymairaSpacing.medium) {
                dirCard("Config", status: report.configDir)
                dirCard("Data", status: report.dataDir)
                dirCard("Cache", status: report.cacheDir)
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

    private func dirCard(_ title: String, status: DirStatus) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textSecondary)
            SymairaBadge(status.exists ? "Exists" : "Missing", tone: status.exists ? .positive : .warning)
            Text(status.path)
                .font(.caption2)
                .foregroundStyle(SymairaTheme.textMuted)
                .lineLimit(1)
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }

    private func configCard(_ config: ConfigStatus) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                SymairaBadge(
                    config.exists ? "Found" : "Not Found",
                    tone: config.exists ? .positive : .neutral
                )
                SymairaBadge(
                    config.parsed ? "Parsed" : "Parse Error",
                    tone: config.parsed ? .positive : .critical
                )
            }
            Spacer()
            Text(config.path)
                .font(.caption2)
                .foregroundStyle(SymairaTheme.textMuted)
                .lineLimit(1)
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
