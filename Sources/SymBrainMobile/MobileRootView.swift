import SwiftUI
import SymairaTheme

struct MobileRootView: View {
    @State private var selectedTab = 0

    var body: some View {
        TabView(selection: $selectedTab) {
            OverviewTab()
                .tabItem {
                    Label("Overview", systemImage: "brain")
                }
                .tag(0)

            ToolsTab()
                .tabItem {
                    Label("Tools", systemImage: "wrench.and.screwdriver")
                }
                .tag(1)

            GuideTab()
                .tabItem {
                    Label("Guide", systemImage: "book")
                }
                .tag(2)
        }
    }
}

// MARK: - Overview Tab

struct OverviewTab: View {
    var body: some View {
        NavigationStack {
            ZStack {
                SymairaBackdrop(gridStyle: .dots)

                ScrollView {
                    VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                        // Header
                        VStack(spacing: SymairaSpacing.medium) {
                            Image(systemName: "brain")
                                .font(.system(size: 48))
                                .foregroundStyle(SymairaTheme.goldPrimary)
                            Text("SymBrain")
                                .font(.largeTitle.bold())
                                .foregroundStyle(SymairaTheme.textPrimary)
                            Text("The portable agent-context layer")
                                .font(.subheadline)
                                .foregroundStyle(SymairaTheme.textSecondary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.top, SymairaSpacing.xLarge)

                        // State cores
                        Text("State Cores")
                            .font(.headline)
                            .foregroundStyle(SymairaTheme.goldPrimary)
                            .padding(.horizontal)

                        coreCard(
                            icon: "lock.shield",
                            name: "Symaira Vault",
                            description: "Credentials and secrets management"
                        )
                        coreCard(
                            icon: "brain.head.profile",
                            name: "Symaira Memory",
                            description: "Persistent semantic memory and entities"
                        )
                        coreCard(
                            icon: "hammer",
                            name: "Symaira Skills",
                            description: "Skill catalog and SSOT"
                        )

                        // What it does
                        Text("How It Works")
                            .font(.headline)
                            .foregroundStyle(SymairaTheme.goldPrimary)
                            .padding(.horizontal)

                        Text("SymBrain multiplexes the three state cores behind one MCP gateway. Each harness connection gets its own profile, controlling exactly what tools that harness can see.")
                            .font(.body)
                            .foregroundStyle(SymairaTheme.textSecondary)
                            .padding(.horizontal)
                            .padding(SymairaSpacing.medium)
                            .glassCard()
                    }
                    .padding(.bottom, SymairaSpacing.xLarge)
                }
            }
            .navigationTitle("SymBrain")
        }
    }

    private func coreCard(icon: String, name: String, description: String) -> some View {
        HStack(spacing: SymairaSpacing.medium) {
            Image(systemName: icon)
                .font(.title2)
                .foregroundStyle(SymairaTheme.goldPrimary)
                .frame(width: 36)
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text(name)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text(description)
                    .font(.caption)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()
        }
        .padding(SymairaSpacing.medium)
        .padding(.horizontal)
        .glassCard()
    }
}

// MARK: - Tools Tab

/// Known Symaira tools — mirrors SymairaToolRegistry.all from SymairaToolKit
/// but defined locally so iOS does not need the macOS-only CLIRunner dependency.
private struct ToolInfo: Identifiable {
    let id: String
    let displayName: String
    let binaryName: String
    let supportsMCP: Bool
}

private let knownTools: [ToolInfo] = [
    .init(id: "symvault", displayName: "Symaira Vault", binaryName: "symvault", supportsMCP: true),
    .init(id: "symmemory", displayName: "Symaira Memory", binaryName: "symmemory", supportsMCP: true),
    .init(id: "symseek", displayName: "Symaira Seek", binaryName: "symseek", supportsMCP: true),
    .init(id: "symfetch", displayName: "Symaira Fetch", binaryName: "symfetch", supportsMCP: true),
    .init(id: "symscope", displayName: "Symaira Scope", binaryName: "symscope", supportsMCP: true),
    .init(id: "symfritz", displayName: "Symaira Fritz", binaryName: "symfritz", supportsMCP: true),
    .init(id: "symprint", displayName: "Symaira Print", binaryName: "symprint", supportsMCP: true),
    .init(id: "symskills", displayName: "Symaira Skills", binaryName: "symskills", supportsMCP: true),
    .init(id: "symguard", displayName: "Symaira Guard", binaryName: "symguard", supportsMCP: false),
    .init(id: "symbrain", displayName: "Symaira Brain", binaryName: "symbrain", supportsMCP: true),
]

struct ToolsTab: View {
    var body: some View {
        NavigationStack {
            ZStack {
                SymairaBackdrop(gridStyle: .dots)

                List {
                    ForEach(knownTools) { tool in
                        HStack {
                            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                                Text(tool.displayName)
                                    .font(.body.weight(.semibold))
                                    .foregroundStyle(SymairaTheme.textPrimary)
                                Text(tool.binaryName)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(SymairaTheme.textMuted)
                            }
                            Spacer()
                            if tool.supportsMCP {
                                SymairaBadge("MCP", tone: .informative)
                            }
                        }
                        .padding(.vertical, SymairaSpacing.xSmall)
                    }
                }
                .scrollContentBackground(.hidden)
                .navigationTitle("Tools")
            }
        }
    }
}

// MARK: - Guide Tab

struct GuideTab: View {
    var body: some View {
        NavigationStack {
            ZStack {
                SymairaBackdrop(gridStyle: .dots)

                ScrollView {
                    VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                        stepRow(1, "Install the CLI", "brew install danieljustus/tap/symbrain")
                        stepRow(2, "Initialize", "symbrain init")
                        stepRow(3, "Check Health", "symbrain doctor")
                        stepRow(4, "Install per Harness", "symbrain install --harness claude --profile personal")
                        stepRow(5, "Verify", "symbrain doctor (confirms installed)")
                        stepRow(6, "Restart Harness", "Reload MCP connections in your harness")
                    }
                    .padding()
                }
            }
            .navigationTitle("Setup Guide")
        }
    }

    private func stepRow(_ number: Int, _ title: String, _ command: String) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack(spacing: SymairaSpacing.small) {
                Text("\(number)")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.black)
                    .frame(width: 24, height: 24)
                    .background(SymairaTheme.goldPrimary, in: Circle())
                Text(title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(SymairaTheme.textPrimary)
            }
            Text(command)
                .font(.caption.monospaced())
                .foregroundStyle(SymairaTheme.textSecondary)
                .padding(SymairaSpacing.small)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
        }
        .padding(SymairaSpacing.medium)
        .glassCard()
    }
}
