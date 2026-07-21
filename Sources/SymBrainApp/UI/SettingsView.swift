import SwiftUI
import SymairaTheme
import SymBrainCore

struct SettingsView: View {
    let client: SymBrainClient

    @AppStorage("binaryPathOverride") private var binaryPathOverride = ""
    @StateObject private var vm: SettingsViewModel

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: SettingsViewModel(client: client))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                headerSection
                binarySection
                versionSection
                updateSection
                aboutSection
            }
            .padding(SymairaSpacing.xLarge)
        }
        .task { await vm.refresh() }
    }

    // MARK: - Header

    private var headerSection: some View {
        Text("Settings")
            .font(.title.bold())
            .foregroundStyle(SymairaTheme.textPrimary)
    }

    // MARK: - Binary Path

    private var binarySection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("Binary Path Override")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)

            Text("Leave empty to auto-detect symbrain from PATH and Homebrew prefixes.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textSecondary)

            TextField("/opt/homebrew/bin/symbrain", text: $binaryPathOverride)
                .textFieldStyle(.roundedBorder)

            Button("Reset to Auto-Detect") {
                binaryPathOverride = ""
            }
            .symairaButtonStyle(.secondary)
        }
        .padding(SymairaSpacing.xLarge)
        .glassCard()
    }

    // MARK: - Version

    private var versionSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("Version")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)

            if let version = vm.versionInfo {
                Grid(alignment: .leading, horizontalSpacing: SymairaSpacing.xLarge, verticalSpacing: SymairaSpacing.small) {
                    GridRow {
                        Text("Version").foregroundStyle(SymairaTheme.textSecondary)
                        Text(version.version).foregroundStyle(SymairaTheme.textPrimary)
                    }
                    if let goVersion = version.goVersion {
                        GridRow {
                            Text("Go").foregroundStyle(SymairaTheme.textSecondary)
                            Text(goVersion).foregroundStyle(SymairaTheme.textPrimary)
                        }
                    }
                    if let os = version.os, let arch = version.arch {
                        GridRow {
                            Text("OS/Arch").foregroundStyle(SymairaTheme.textSecondary)
                            Text("\(os)/\(arch)").foregroundStyle(SymairaTheme.textPrimary)
                        }
                    }
                    GridRow {
                        Text("Schema").foregroundStyle(SymairaTheme.textSecondary)
                        Text("\(version.schemaVersion)").foregroundStyle(SymairaTheme.textPrimary)
                    }
                }
                .font(.caption.monospaced())
            } else {
                SymairaLoadingState("Loading version...")
            }
        }
        .padding(SymairaSpacing.xLarge)
        .glassCard()
    }

    // MARK: - Update

    private var updateSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("Update")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)

            if let info = vm.updateInfo {
                SymairaNotice(title: nil, message: info, tone: .informative)
            }

            Button(action: { Task { await vm.checkForUpdate() } }) {
                Label("Check for Updates", systemImage: "arrow.up.circle")
            }
            .symairaButtonStyle(.secondary)
        }
        .padding(SymairaSpacing.xLarge)
        .glassCard()
    }

    // MARK: - About

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("About")
                .font(.headline)
                .foregroundStyle(SymairaTheme.goldPrimary)

            Text("SymBrain is the portable agent-context layer for the Symaira ecosystem. It multiplexes state cores behind one MCP gateway.")
                .font(.body)
                .foregroundStyle(SymairaTheme.textSecondary)

            Text("Daemon supervision (symbrain serve) is coming in a future release.")
                .font(.caption)
                .foregroundStyle(SymairaTheme.textMuted)
                .italic()
        }
        .padding(SymairaSpacing.xLarge)
        .glassCard()
    }
}
