import SwiftUI
import SymairaTheme
import SymBrainCore

struct ContentView: View {
    let client: SymBrainClient

    @State private var displayMode: DisplayMode = .dashboard

    enum DisplayMode: String, CaseIterable {
        case dashboard = "Dashboard"
        case profiles = "Profiles"
        case harnesses = "Harnesses"
        case sync = "Sync"
        case audit = "Audit"
        case settings = "Settings"

        var systemImage: String {
            switch self {
            case .dashboard: "gauge.with.dots.needle.33percent"
            case .profiles: "person.crop.rectangle.stack"
            case .harnesses: "terminal"
            case .sync: "arrow.triangle.2.circlepath"
            case .audit: "doc.text.magnifyingglass"
            case .settings: "gearshape"
            }
        }
    }

    var body: some View {
        NavigationSplitView {
            List(DisplayMode.allCases, id: \.self, selection: $displayMode) { mode in
                Button(action: { displayMode = mode }) {
                    HStack {
                        Image(systemName: mode.systemImage)
                            .frame(width: 20)
                        Text(mode.rawValue)
                        if mode == .sync {
                            Spacer()
                            SymairaBadge("Planned", tone: .neutral)
                        }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .listStyle(.sidebar)
            .buttonStyle(.plain)
            .frame(minWidth: 220, idealWidth: 240)
            .background(.clear)
            .navigationTitle("SymBrain")
        } detail: {
            SymairaScreen {
                switch displayMode {
                case .dashboard:
                    DashboardView(client: client)
                case .profiles:
                    ProfilesView(client: client)
                case .harnesses:
                    HarnessesView(client: client)
                case .sync:
                    SyncView()
                case .audit:
                    AuditView(client: client)
                case .settings:
                    SettingsView(client: client)
                }
            }
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 900, minHeight: 580)
    }
}

// MARK: - SymairaScreen

/// Full-window screen wrapper with backdrop and padding.
struct SymairaScreen<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        ZStack {
            SymairaBackdrop(gridStyle: .dots)
            content
                .padding(SymairaSpacing.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }
}
