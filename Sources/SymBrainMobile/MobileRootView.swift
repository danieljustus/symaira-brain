import SwiftUI
import SymairaTheme
import SymairaToolKit
import UIKit

/// The read-only iOS companion: what Symaira Brain is, which Symaira tools
/// exist, and how to set Brain up on a Mac. Native `TabView` (IOS-1).
struct MobileRootView: View {
    enum Tab: String, CaseIterable, Identifiable, Hashable {
        case overview, tools, guide

        var id: String { rawValue }

        var title: String {
            switch self {
            case .overview: "Overview"
            case .tools: "Tools"
            case .guide: "Setup"
            }
        }

        var icon: SymairaIcon {
            switch self {
            case .overview: .productBrain
            case .tools: .tools
            case .guide: .checklist
            }
        }
    }

    @State private var tab: Tab = .overview
    @State private var topography = SymairaTopoModel()

    var body: some View {
        TabView(selection: $tab) {
            ForEach(Tab.allCases) { item in
                NavigationStack {
                    page(item)
                        // Topography behind the main overview only (MOTION-1, IOS-5).
                        .symairaTopography(item == .overview ? topography : nil)
                        .toolbar(.hidden, for: .navigationBar)
                }
                // The tab bar renders only Image, so the icon is a still (IOS-6).
                .tabItem { Label { Text(item.title) } icon: { Image(symairaIcon: item.icon) } }
                .tag(item)
            }
        }
    }

    @ViewBuilder
    private func page(_ item: Tab) -> some View {
        switch item {
        case .overview: OverviewTab()
        case .tools: ToolsTab()
        case .guide: GuideTab()
        }
    }
}

// MARK: - Overview

struct OverviewTab: View {
    var body: some View {
        SymairaPage(title: "Symaira Brain", subtitle: "The portable agent-context layer") {
            VStack(alignment: .leading, spacing: SymairaSpacing.large) {
                SymairaNotice(
                    message: "This companion is read-only. Manage profiles, harnesses and credentials in Symaira Brain on your Mac.",
                    tone: .informative
                )
                SymairaFormSection("State Cores") {
                    SymairaFormRow("Vault", description: "Credentials and secrets, in their own process")
                    SymairaFormDivider()
                    SymairaFormRow("Memory", description: "Facts, rules and entities your agents keep")
                    SymairaFormDivider()
                    SymairaFormRow("Skills", description: "One skill library for every harness")
                }
                SymairaFormSection(
                    "How It Works",
                    footer: "A harness such as Claude Code or Codex connects to one MCP gateway instead of each tool separately."
                ) {
                    Text("Symaira Brain puts the state cores behind one MCP gateway. Every harness connection gets its own profile, which decides exactly which tools that harness can see.")
                        .symairaText(.body)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.vertical, SymairaSpacing.small)
                }
            }
        }
    }
}

// MARK: - Tools

/// Active Symaira tools from the shared registry.
struct ToolsTab: View {
    var body: some View {
        SymairaPage(title: "Tools", subtitle: "Active tools in the Symaira family") {
            SymairaFormSection(footer: "MCP marks tools that an agent harness can call through a profile.") {
                ForEach(Array(SymairaToolRegistry.active.enumerated()), id: \.element.id) { index, tool in
                    if index > 0 { SymairaFormDivider() }
                    SymairaFormRow(tool.displayName, description: tool.binaryName) {
                        if tool.supportsMCP {
                            SymairaBadge("MCP", tone: .informative)
                        }
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }
}

// MARK: - Setup guide

struct GuideTab: View {
    private struct Step: Identifiable {
        let id: Int
        let title: String
        let command: String?
        let note: String?
    }

    private let steps = [
        Step(id: 1, title: "Install the CLI", command: "brew install danieljustus/tap/symbrain", note: nil),
        Step(id: 2, title: "Initialize", command: "symbrain init", note: nil),
        Step(id: 3, title: "Check Health", command: "symbrain doctor", note: nil),
        Step(id: 4, title: "Connect a Harness", command: "symbrain install --harness claude --profile personal", note: nil),
        Step(id: 5, title: "Verify", command: "symbrain doctor", note: "The harness now shows as installed."),
        Step(id: 6, title: "Restart the Harness", command: nil, note: "Reload the MCP connections in your harness."),
    ]

    var body: some View {
        SymairaPage(title: "Setup", subtitle: "Six steps on your Mac") {
            SymairaFormSection(footer: "Run the commands in Terminal on the Mac where your agents work.") {
                ForEach(steps) { step in
                    if step.id > 1 { SymairaFormDivider() }
                    stepRow(step)
                }
            }
        }
    }

    private func stepRow(_ step: Step) -> some View {
        HStack(alignment: .top, spacing: SymairaSpacing.medium) {
            Text("\(step.id)")
                .font(SymairaTypography.readout)
                .foregroundStyle(SymairaTokens.ink3)
                .frame(minWidth: 24, alignment: .leading)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text(step.title).symairaText(.bodyEmphasized)
                if let command = step.command {
                    Text(verbatim: command)
                        .symairaText(.monoSmall)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let note = step.note {
                    Text(note).symairaText(.caption)
                }
            }
            Spacer(minLength: 0)
            if let command = step.command {
                SymairaIconButton("Copy \(step.title) Command", icon: .copy, size: 18) {
                    UIPasteboard.general.string = command
                }
                .frame(minWidth: 44, minHeight: 44)
            }
        }
        .padding(.vertical, SymairaSpacing.small)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Step \(step.id), \(step.title)")
    }
}
