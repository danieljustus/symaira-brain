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
        case memory = "Memory"
        case vault = "Vault"
        case skills = "Skills"
        case settings = "Settings"

        var systemImage: String {
            switch self {
            case .dashboard: "gauge.with.dots.needle.33percent"
            case .profiles: "person.crop.rectangle.stack"
            case .harnesses: "terminal"
            case .sync: "arrow.triangle.2.circlepath"
            case .audit: "doc.text.magnifyingglass"
            case .memory: "brain"
            case .vault: "lock.square.stack"
            case .skills: "square.stack.3d.up"
            case .settings: "gearshape"
            }
        }

        /// Sidebar grouping: the broker screens SymBrain owns itself, and the
        /// modules it fronts for the other Symaira tools.
        static let brokerModes: [DisplayMode] = [
            .dashboard, .profiles, .harnesses, .sync, .audit,
        ]
        static let moduleModes: [DisplayMode] = [.memory, .vault, .skills]
        static let systemModes: [DisplayMode] = [.settings]
    }

    var body: some View {
        NavigationSplitView {
            List(selection: $displayMode) {
                Section("Broker") {
                    ForEach(DisplayMode.brokerModes, id: \.self, content: sidebarRow)
                }
                Section("Modules") {
                    ForEach(DisplayMode.moduleModes, id: \.self, content: sidebarRow)
                }
                Section {
                    ForEach(DisplayMode.systemModes, id: \.self, content: sidebarRow)
                }
            }
            .scrollContentBackground(.hidden)
            .listStyle(.sidebar)
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
                    SyncView(client: client)
                case .audit:
                    AuditView(client: client)
                case .memory:
                    MemoryView()
                case .vault:
                    VaultView()
                case .skills:
                    SkillsView()
                case .settings:
                    SettingsView(client: client)
                }
            }
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 900, minHeight: 580)
        .background(WindowMinSizeConfigurator(minSize: CGSize(width: 900, height: 580)))
    }

    private func sidebarRow(_ mode: DisplayMode) -> some View {
        HStack {
            Image(systemName: mode.systemImage)
                .frame(width: 20)
            Text(mode.rawValue)
        }
        .tag(mode)
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
        .frame(minWidth: 680, minHeight: 580, alignment: .topLeading)
    }
}

// MARK: - Window Minimum Size Configurator (#308)

#if os(macOS)
import AppKit

/// Ensures the underlying AppKit `NSWindow` enforces minimum dimensions so
/// the window can never collapse into a remnant during view transitions.
private struct WindowMinSizeConfigurator: NSViewRepresentable {
    let minSize: CGSize

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            if let window = view.window {
                window.minSize = minSize
                window.contentMinSize = minSize
            }
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        DispatchQueue.main.async {
            if let window = nsView.window {
                window.minSize = minSize
                window.contentMinSize = minSize
            }
        }
    }
}
#endif
