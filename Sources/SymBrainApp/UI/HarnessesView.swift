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

    // #75: Install-overwrite confirmation
    @State private var showInstallConfirmation = false
    @State private var pendingInstallHarness: String?
    @State private var pendingInstallProfile: String?

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
                client: client,
                profiles: vm.profiles
            )
        }
        // #75: Confirmation before overwriting an installed harness config
        .alert("Install Harness", isPresented: $showInstallConfirmation) {
            Button("Cancel", role: .cancel) {
                pendingInstallHarness = nil
                pendingInstallProfile = nil
            }
            Button("Install", role: .destructive) {
                if let h = pendingInstallHarness, let p = pendingInstallProfile {
                    selectedHarness = h
                    selectedProfile = p
                    isInstallAction = true
                    Task { await vm.install(harness: h, profile: p, dryRun: false) }
                }
                pendingInstallHarness = nil
                pendingInstallProfile = nil
            }
        } message: {
            if let h = pendingInstallHarness,
               let p = pendingInstallProfile,
               let status = vm.harnesses.first(where: { $0.name == h })
            {
                Text("Installing profile \"\(p)\" to harness \"\(h)\" will overwrite the existing configuration at:\n\(status.configPath)\n\nThis action cannot be undone.")
            }
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

            // #75: Install menu — confirm overwrite when already installed
            Menu {
                ForEach(vm.profiles, id: \.name) { profile in
                    Button("\(profile.name)") {
                        if status?.installed == true {
                            pendingInstallHarness = name
                            pendingInstallProfile = profile.name
                            showInstallConfirmation = true
                        } else {
                            selectedHarness = name
                            selectedProfile = profile.name
                            isInstallAction = true
                            Task { await vm.install(harness: name, profile: profile.name, dryRun: false) }
                        }
                    }
                }
            } label: {
                Label("Install", systemImage: "arrow.down.to.line")
                    .font(.caption)
            }
            .menuStyle(.borderlessButton)
            .frame(minWidth: 90)

            // Dry-run preview for install
            Button(action: {
                selectedHarness = name
                selectedProfile = nil  // #73: Clear stale profile so the picker shows
                isInstallAction = true
                showDryRunSheet = true
            }) {
                Label("Dry Run", systemImage: "doc.text.magnifyingglass")
                    .font(.caption)
            }
            .buttonStyle(.plain)
            .foregroundStyle(SymairaTheme.textSecondary)

            // Uninstall controls (only when installed)
            if status?.installed == true {
                // #74: Dry Run for Uninstall
                Button(action: {
                    selectedHarness = name
                    isInstallAction = false
                    showDryRunSheet = true
                }) {
                    Label("Dry Run", systemImage: "doc.text.magnifyingglass")
                        .font(.caption)
                }
                .buttonStyle(.plain)
                .foregroundStyle(SymairaTheme.textSecondary)

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
    let initialProfile: String
    let isInstall: Bool
    let client: SymBrainClient
    let profiles: [ProfileSummary]

    @Environment(\.dismiss) private var dismiss
    @State private var output: String = ""
    @State private var isLoading = false
    @State private var hasRun = false
    @State private var selectedProfile: String
    @State private var errorMessage: String?

    init(harness: String, profile: String, isInstall: Bool, client: SymBrainClient, profiles: [ProfileSummary]) {
        self.harness = harness
        self.initialProfile = profile
        self.isInstall = isInstall
        self.client = client
        self.profiles = profiles
        // #73: For install, default to first available profile if none specified
        let initial = profile.isEmpty ? (profiles.first?.name ?? "") : profile
        _selectedProfile = State(initialValue: initial)
    }

    var body: some View {
        VStack(spacing: SymairaSpacing.xLarge) {
            Text("Dry Run: \(isInstall ? "Install" : "Uninstall")")
                .font(.title2.bold())
                .foregroundStyle(SymairaTheme.textPrimary)

            Text("Harness: \(harness)")
                .font(.subheadline)
                .foregroundStyle(SymairaTheme.textSecondary)

            if isInstall && !hasRun && !isLoading {
                // #73: Profile picker for install dry run
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    Text("Profile")
                        .font(.headline)
                        .foregroundStyle(SymairaTheme.textSecondary)

                    if profiles.isEmpty {
                        Text("No profiles available. Create one in the Profiles tab.")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    } else {
                        Picker("Profile", selection: $selectedProfile) {
                            ForEach(profiles, id: \.name) { p in
                                Text(p.name).tag(p.name)
                            }
                        }
                        .pickerStyle(.menu)
                        .labelsHidden()
                    }
                }

                if let error = errorMessage {
                    SymairaNotice(title: "Error", message: error, tone: .critical)
                }

                HStack {
                    Button("Cancel") { dismiss() }
                        .symairaButtonStyle(.secondary)
                    Spacer()
                    Button("Run Dry Run") {
                        guard !selectedProfile.isEmpty else {
                            errorMessage = "Please select a profile."
                            return
                        }
                        isLoading = true
                        Task { await runDryRun() }
                    }
                    .symairaButtonStyle(.primary)
                    .disabled(profiles.isEmpty || selectedProfile.isEmpty)
                }
            } else if isLoading {
                SymairaLoadingState("Running dry run...")
            } else if hasRun {
                ScrollView {
                    Text(output)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(SymairaTheme.textSecondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(SymairaSpacing.medium)
                        .glassCard()
                }

                Button("Done") { dismiss() }
                    .symairaButtonStyle(.secondary)
            }
        }
        .padding(SymairaSpacing.xLarge)
        .frame(width: 560, height: 420)
        .task {
            // #74: For uninstall, run immediately without profile picker
            if !isInstall {
                isLoading = true
                await runDryRun()
            }
        }
    }

    private func runDryRun() async {
        do {
            if isInstall {
                output = try await client.install(harness: harness, profile: selectedProfile, dryRun: true)
            } else {
                output = try await client.uninstall(harness: harness, dryRun: true)
            }
        } catch {
            output = "Error: \(error.localizedDescription)"
        }
        isLoading = false
        hasRun = true
    }
}
