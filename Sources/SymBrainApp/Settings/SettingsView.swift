import SwiftUI
import SymairaTheme
import SymBrainCore

/// The native Settings window (⌘,): appearance, CLI connection, updates, about.
struct SettingsView: View {
    @EnvironmentObject private var clientHolder: ClientHolder

    var body: some View {
        // Keyed on the client so version and update state follow a new CLI path.
        SettingsForm(client: clientHolder.client)
            .id(clientHolder.generation)
    }
}

private struct SettingsForm: View {
    @EnvironmentObject private var status: AppStatus
    @AppStorage(PreferenceKey.appearance) private var appearance: SymairaAppearance = .system
    @AppStorage(PreferenceKey.binaryPathOverride) private var binaryPathOverride = ""
    @StateObject private var vm: SettingsViewModel
    @State private var pathDraft = ""
    @FocusState private var pathFocused: Bool

    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: SettingsViewModel(client: client))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.large) {
                SymairaFormSection("Appearance") {
                    SymairaFormRow("Color Scheme") {
                        SymairaAppearancePicker(selection: $appearance)
                            .frame(width: 300)
                    }
                }
                connection
                updates
                about
            }
            .padding(SymairaSpacing.xLarge)
        }
        .background(SymairaTokens.canvas)
        .onAppear {
            pathDraft = binaryPathOverride
            appearance.apply()
        }
        .onChange(of: appearance) { _, value in value.apply() }
        .task { await vm.refresh() }
    }

    // MARK: - Connection

    private var connection: some View {
        SymairaFormSection(
            "Connection",
            footer: "Leave the path empty to find symbrain on PATH and in the Homebrew prefixes. Applying a path reloads every page."
        ) {
            SymairaFormRow("CLI Status") {
                SymairaStatusLabel(status.label, tone: status.tone)
            }
            SymairaFormDivider()
            SymairaFormRow("CLI Path") {
                TextField("Auto-detect", text: $pathDraft)
                    .textFieldStyle(.symaira(isFocused: pathFocused))
                    .focused($pathFocused)
                    .frame(width: 280)
                    .onSubmit(applyPath)
            }
            HStack(spacing: SymairaSpacing.medium) {
                Spacer()
                Button("Use Auto-Detect") {
                    pathDraft = ""
                    applyPath()
                }
                .symairaButtonStyle(.secondary)
                .disabled(binaryPathOverride.isEmpty)
                Button("Apply Path", action: applyPath)
                    .symairaButtonStyle(.secondary)
                    .disabled(pathDraft.trimmingCharacters(in: .whitespaces) == binaryPathOverride)
            }
            .padding(.bottom, SymairaSpacing.small)
        }
    }

    /// Writing the key rebuilds the client in the app scene; writing per
    /// keystroke would remount the workspace on every character.
    private func applyPath() {
        binaryPathOverride = pathDraft.trimmingCharacters(in: .whitespaces)
    }

    // MARK: - Updates

    private var updates: some View {
        SymairaFormSection("Updates") {
            SymairaFormRow("Check on Launch", description: "Looks for a newer Symaira Brain release on GitHub when the app starts.") {
                Toggle(isOn: Binding(
                    get: { vm.autoPrefs.autoCheckEnabled },
                    set: { vm.autoPrefs.autoCheckEnabled = $0 }
                )) { EmptyView() }
                .toggleStyle(.symairaSwitch)
                .accessibilityLabel("Check on Launch")
            }
            SymairaFormDivider()
            SymairaFormRow("Status", description: vm.updateInfo) {
                updateActions
            }
            if case .installing(let progress) = vm.updateStatus {
                ProgressView(value: progress)
                    .progressViewStyle(SymairaProgressStyle())
                    .accessibilityLabel("Installing update")
                    .padding(.bottom, SymairaSpacing.small)
            }
        }
    }

    @ViewBuilder
    private var updateActions: some View {
        switch vm.updateStatus {
        case .available(let release):
            HStack(spacing: SymairaSpacing.medium) {
                Button("Skip This Version") { vm.skipUpdate(release) }
                    .symairaButtonStyle(.secondary)
                Button("Install \(release.tagName)") { Task { await vm.installUpdate(release) } }
                    .symairaButtonStyle(.primary)
            }
        case .readyToRelaunch:
            Button("Relaunch to Update") { vm.relaunchAfterUpdate() }
                .symairaButtonStyle(.primary)
        case .installing:
            Text("Installing…").symairaText(.secondary)
        default:
            if vm.isLoading {
                Text("Checking…").symairaText(.secondary)
            } else {
                Button("Check for Updates") { Task { await vm.checkForUpdate() } }
                    .symairaButtonStyle(.secondary)
            }
        }
    }

    // MARK: - About

    private var about: some View {
        SymairaFormSection(
            "About",
            footer: "Symaira Brain is the portable agent-context layer: one MCP gateway in front of credentials, memory and skills, with a profile per harness."
        ) {
            DetailRow(label: "App", value: AppVersionInfo.current())
            if let version = vm.versionInfo {
                DetailRow(label: "symbrain CLI", value: version.version)
                if let os = version.os, let arch = version.arch {
                    DetailRow(label: "Platform", value: "\(os)/\(arch)")
                }
                DetailRow(label: "JSON Schema", value: "\(version.schemaVersion)")
            } else if let error = vm.errorMessage {
                DetailRow(label: "symbrain CLI", value: error)
            } else {
                DetailRow(label: "symbrain CLI", value: "Loading…")
            }
        }
    }
}
