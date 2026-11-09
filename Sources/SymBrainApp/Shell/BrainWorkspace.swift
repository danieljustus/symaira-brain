import SwiftUI
import SymairaTheme
import SymBrainCore

/// The main window: the shared Symaira workspace with the seven destinations.
struct BrainWorkspace: View {
    @EnvironmentObject private var clientHolder: ClientHolder
    @EnvironmentObject private var status: AppStatus
    @Environment(\.openSettings) private var openSettings

    @SceneStorage("brain.destination") private var destination: Destination = .overview
    @State private var sidebar = SymairaSidebarState()
    @State private var topography = SymairaTopoModel()

    var body: some View {
        SymairaWorkspace(
            sidebar: $sidebar,
            // Topography belongs behind the overview only (SHELL-8, MOTION-1).
            topography: destination == .overview ? topography : nil,
            navigation: { showsLabels in sidebarContent(showsLabels: showsLabels) },
            content: { page }
        )
        .tint(SymairaTokens.selected)
        .toolbar {
            SymairaWorkspaceToolbar(sidebar: $sidebar) { openSettings() }
        }
        .focusedSceneValue(\.brainDestination, $destination)
        .task(id: clientHolder.generation) {
            await status.refresh(client: clientHolder.client)
        }
    }

    // MARK: - Sidebar

    private func sidebarContent(showsLabels: Bool) -> some View {
        VStack(alignment: showsLabels ? .leading : .center, spacing: SymairaSpacing.large) {
            HStack(spacing: SymairaSpacing.small) {
                SymairaIconView(.productBrain)
                    .foregroundStyle(SymairaTokens.ink2)
                    .frame(width: 32, height: 36)
                    .accessibilityHidden(true)
                if showsLabels {
                    Text("Symaira Brain")
                        .symairaText(.heading)
                        .lineLimit(1)
                }
            }
            .padding(.bottom, SymairaSpacing.small)

            ScrollView {
                VStack(spacing: SymairaSpacing.xSmall) {
                    ForEach(Destination.allCases) { item in
                        SymairaNavigationButton(
                            item.title, icon: item.icon,
                            isSelected: destination == item, showsLabel: showsLabels
                        ) {
                            destination = item
                        }
                        .accessibilityIdentifier("brain-nav-\(item.id)")
                    }
                }
                .padding(3)
            }

            Divider().overlay(SymairaTokens.line)
            if showsLabels {
                SymairaStatusLabel(status.label, tone: status.tone)
                    .padding(.horizontal, SymairaSpacing.small)
                    .accessibilityIdentifier("brain-cli-status")
            } else {
                SymairaStatusDot(tone: status.tone, accessibilityLabel: status.label)
                    .help(status.label)
            }
        }
        .padding(SymairaSpacing.medium)
        .frame(maxHeight: .infinity)
    }

    // MARK: - Content

    /// Keyed on the client generation: a changed CLI path in Settings remounts
    /// the pages with fresh view models instead of requiring a relaunch.
    private var page: some View {
        Group {
            switch destination {
            case .overview: OverviewPage(client: clientHolder.client, destination: $destination)
            case .harnesses: HarnessesPage(client: clientHolder.client)
            case .profiles: ProfilesPage(client: clientHolder.client)
            case .memory: MemoryPage(client: clientHolder.client)
            case .skills: SkillsPage(client: clientHolder.client)
            case .credentials: CredentialsPage()
            case .activity: ActivityPage(client: clientHolder.client)
            }
        }
        .id(clientHolder.generation)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("brain-content-\(destination.id)")
    }
}
