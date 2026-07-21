import SwiftUI
import SymairaTheme
import SymBrainCore

struct HarnessesView: View {
    let client: SymBrainClient

    @StateObject private var vm: HarnessesViewModel
    @State private var showDryRunSheet = false
    @State private var selectedHarness: String?
    @State private var selectedProfile: String?
    @State private var isInstallAction = true

    private let allHarnesses = ["claude", "claude-desktop", "cursor", "opencode", "codex", "gemini"]

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: HarnessesViewModel(client: client))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                headerSection

                if vm.isLoading {
                    SymairaLoadingState("Loading harnesses...")
                } else if let error = vm.errorMessage {
                    SymairaNotice(title: "Error", message: error, tone: .critical)
                } else {
                    harnessListSection
                }

                if let result = vm.operationResult {
                    SymairaNotice(title: "Result", message: result, tone: .informative)
                }
            }
            .padding(SymairaSpacing.xLarge)
        }
        .task { await vm.refresh() }
        .sheet(isPresented: $showDryRunSheet) {
            DryRunSheet(
                harness: selectedHarness ?? "",
                profile: selectedProfile ?? "",
                isInstall: isInstallAction,
                client: client
            )
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text("Harnesses")
                .font(.title.bold())
                .foregroundStyle(SymairaTheme.textPrimary)
            Text("Install or uninstall symbrain as an MCP server in each harness")
                .font(.subheadline)
                .foregroundStyle(SymairaTheme.textSecondary)
        }
    }

    // MARK: - Harness List

    private var harnessListSection: some View {
        VStack(spacing: SymairaSpacing.medium) {
            ForEach(allHarnesses, id: \.self) { name in
                harnessRow(name)
            }
        }
    }

    private func harnessRow(_ name: String) -> some View {
        let status = vm.harnesses.first { $0.name == name }

        return HStack {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                HStack {
                    Text(name)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(SymairaTheme.textPrimary)
                    if let status {
                        SymairaBadge(status.installed ? "Installed" : "Not Installed", tone: status.installed ? .positive : .neutral)
                    }
                }
                if let status {
                    Text(status.configPath)
                        .font(.caption2)
                        .foregroundStyle(SymairaTheme.textMuted)
                        .lineLimit(1)
                }
            }

            Spacer()

            // Install button
            Menu {
                ForEach(vm.profiles, id: \.name) { profile in
                    Button("\(profile.name)") {
                        selectedHarness = name
                        selectedProfile = profile.name
                        isInstallAction = true
                        Task { await vm.install(harness: name, profile: profile.name, dryRun: false) }
                    }
                }
            } label: {
                Label("Install", systemImage: "arrow.down.to.line")
                    .font(.caption)
            }
            .menuStyle(.borderlessButton)
            .frame(minWidth: 90)

            // Dry-run preview
            Button(action: {
                selectedHarness = name
                isInstallAction = true
                showDryRunSheet = true
            }) {
                Label("Dry Run", systemImage: "doc.text.magnifyingglass")
                    .font(.caption)
            }
            .buttonStyle(.plain)
            .foregroundStyle(SymairaTheme.textSecondary)

            // Uninstall button
            if status?.installed == true {
                Button(action: {
                    Task { await vm.uninstall(harness: name, dryRun: false) }
                }) {
                    Label("Uninstall", systemImage: "arrow.up.from.line")
                        .font(.caption)
                }
                .buttonStyle(.plain)
                .foregroundStyle(SymairaTheme.critical)
            }
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }
}

// MARK: - Dry Run Sheet

struct DryRunSheet: View {
    let harness: String
    let profile: String
    let isInstall: Bool
    let client: SymBrainClient

    @Environment(\.dismiss) private var dismiss
    @State private var output: String = ""
    @State private var isLoading = true

    var body: some View {
        VStack(spacing: SymairaSpacing.xLarge) {
            Text("Dry Run: \(isInstall ? "Install" : "Uninstall")")
                .font(.title2.bold())
                .foregroundStyle(SymairaTheme.textPrimary)

            Text("Harness: \(harness)")
                .font(.subheadline)
                .foregroundStyle(SymairaTheme.textSecondary)

            if isLoading {
                SymairaLoadingState("Running dry run...")
            } else {
                ScrollView {
                    Text(output)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(SymairaTheme.textSecondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(SymairaSpacing.medium)
                        .glassCard()
                }
            }

            Button("Done") { dismiss() }
                .symairaButtonStyle(.secondary)
        }
        .padding(SymairaSpacing.xLarge)
        .frame(width: 560, height: 420)
        .task {
            do {
                if isInstall {
                    output = try await client.install(harness: harness, profile: profile, dryRun: true)
                } else {
                    output = try await client.uninstall(harness: harness, dryRun: true)
                }
            } catch {
                output = "Error: \(error.localizedDescription)"
            }
            isLoading = false
        }
    }
}
