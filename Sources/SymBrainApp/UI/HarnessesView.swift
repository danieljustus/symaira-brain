import SwiftUI
import SymairaTheme
import SymBrainCore

struct HarnessesView: View {
    let client: SymBrainClient

    @StateObject private var vm: HarnessesViewModel
    // #83: Sheets are presented via .sheet(item:) with a request value captured
    // at the exact moment of the button tap. With .sheet(isPresented:) SwiftUI
    // evaluates the content closure before the @State update from the tap is
    // applied, so the first sheet of a session was built with an empty harness.
    @State private var dryRunRequest: DryRunRequest?

    // #75: Install-overwrite confirmation
    @State private var pendingInstall: InstallConfirmation?

    // #149: Uninstall confirmation — uninstall rewrites the harness config.
    @State private var pendingUninstall: UninstallConfirmation?

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
                    VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                        SymairaNotice(title: "Error", message: error, tone: .critical)
                        Button(action: { Task { await vm.refresh() } }) {
                            Label("Retry", systemImage: "arrow.clockwise")
                        }
                        .symairaButtonStyle(.primary)
                        .accessibilityLabel("Retry")
                        if let detail = vm.errorDetail {
                            SymairaNotice(title: "Details", message: detail, tone: .neutral)
                        }
                    }
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
        .sheet(item: $dryRunRequest) { request in
            DryRunSheet(
                harness: request.harness,
                profile: request.profile ?? "",
                isInstall: request.isInstall,
                client: client,
                profiles: vm.profiles
            )
        }
        // #75: Confirmation before overwriting an installed harness config.
        // The request value is passed via `presenting:` so the alert always
        // reads the harness/profile captured at the moment of the menu tap.
        .alert(
            "Install Harness",
            isPresented: Binding(
                get: { pendingInstall != nil },
                set: { if !$0 { pendingInstall = nil } }
            ),
            presenting: pendingInstall
        ) { install in
            Button("Cancel", role: .cancel) {
                pendingInstall = nil
            }
            Button("Install", role: .destructive) {
                Task { await vm.install(harness: install.harness, profile: install.profile, dryRun: false) }
                pendingInstall = nil
            }
        } message: { install in
            if let status = vm.harnesses.first(where: { $0.name == install.harness }) {
                Text("Installing profile \"\(install.profile)\" to harness \"\(install.harness)\" will overwrite the existing configuration at:\n\(status.configPath)\n\nThis action cannot be undone.")
            }
        }
        // #149: Confirmation before uninstalling — uninstall rewrites the
        // harness config. Mirrors the install confirmation (#75) so cancel
        // leaves the config untouched.
        .alert(
            "Uninstall Harness",
            isPresented: Binding(
                get: { pendingUninstall != nil },
                set: { if !$0 { pendingUninstall = nil } }
            ),
            presenting: pendingUninstall
        ) { uninstall in
            Button("Cancel", role: .cancel) {
                pendingUninstall = nil
            }
            Button("Uninstall", role: .destructive) {
                Task { await vm.uninstall(harness: uninstall.harness, dryRun: false) }
                pendingUninstall = nil
            }
        } message: { uninstall in
            if let status = vm.harnesses.first(where: { $0.name == uninstall.harness }) {
                Text("Uninstalling harness \"\(uninstall.harness)\" will rewrite the configuration at:\n\(status.configPath)\n\nThis action cannot be undone.")
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

            // #151: Fixed-width trailing region so the Install control starts
            // at the same x on every harness row; the inner Spacer absorbs the
            // width difference between installed and uninstalled rows.
            HStack(spacing: SymairaSpacing.medium) {
                // #75: Install menu — confirm overwrite when already installed
                Menu {
                    ForEach(vm.profiles, id: \.name) { profile in
                        Button("\(profile.name)") {
                            if status?.installed == true {
                                pendingInstall = InstallConfirmation(harness: name, profile: profile.name)
                            } else {
                                Task { await vm.install(harness: name, profile: profile.name, dryRun: false) }
                            }
                        }
                    }
                } label: {
                    Label("Install", systemImage: "arrow.down.to.line")
                        .font(.caption)
                }
                // #151: bordered style with intrinsic width so the hit area is
                // compact and visible (was an oversized invisible target with
                // the label ~470pt away from its disclosure chevron).
                .menuStyle(.button)
                .buttonStyle(.bordered)
                .fixedSize()

                Spacer(minLength: SymairaSpacing.medium)

                // Dry-run preview for install
                Button(action: {
                    // #83: capture harness at tap time; profile is nil so the
                    // sheet's profile picker is shown (#73).
                    dryRunRequest = DryRunRequest(harness: name, profile: nil, isInstall: true)
                }) {
                    Label("Dry Run", systemImage: "doc.text.magnifyingglass")
                        .font(.caption)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Dry Run")
                .foregroundStyle(SymairaTheme.textSecondary)

                // Uninstall controls (only when installed)
                if status?.installed == true {
                    // #74: Dry Run for Uninstall
                    Button(action: {
                        dryRunRequest = DryRunRequest(harness: name, profile: nil, isInstall: false)
                    }) {
                        Label("Dry Run", systemImage: "doc.text.magnifyingglass")
                            .font(.caption)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Dry Run")
                    .foregroundStyle(SymairaTheme.textSecondary)

                    // #149: Uninstall is gated behind a confirmation alert
                    // (uninstall rewrites the harness config).
                    Button(action: {
                        pendingUninstall = UninstallConfirmation(harness: name)
                    }) {
                        Label("Uninstall", systemImage: "arrow.up.from.line")
                            .font(.caption)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Uninstall")
                    .foregroundStyle(SymairaTheme.critical)
                }
            }
            .frame(width: 380, alignment: .trailing)
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }
}

// MARK: - Sheet / Alert Requests (#83)

/// A dry-run request captured at the exact moment of the button tap.
/// Presented via `.sheet(item:)` so the sheet content is always built from
/// this value, never from externally captured, potentially stale `@State`.
private struct DryRunRequest: Identifiable {
    let id = UUID()
    let harness: String
    /// `nil` for an install dry run → the sheet shows its profile picker (#73).
    let profile: String?
    let isInstall: Bool
}

/// A pending install-overwrite confirmation (#75), captured at menu-tap time
/// and handed to the alert via `presenting:`.
private struct InstallConfirmation: Identifiable {
    let id = UUID()
    let harness: String
    let profile: String
}

/// A pending uninstall confirmation (#149), captured at button-tap time and
/// handed to the alert via `presenting:`.
private struct UninstallConfirmation: Identifiable {
    let id = UUID()
    let harness: String
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
                        .keyboardShortcut(.cancelAction)
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
                    .keyboardShortcut(.defaultAction)
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
