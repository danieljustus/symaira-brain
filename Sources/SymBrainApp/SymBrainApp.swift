import SwiftUI
import SymairaTheme
import SymairaUpdateCheck
import SymBrainCore

/// Holds the `SymBrainClient` so a changed CLI path override rebuilds it at
/// runtime. `generation` changes with every rebuild; the workspace keys its
/// pages on it so their view models pick up the new client.
@MainActor
final class ClientHolder: ObservableObject {
    @Published private(set) var client: SymBrainClient
    @Published private(set) var generation = 0

    init(binaryPathOverride: String) {
        client = Self.makeClient(binaryPathOverride)
    }

    func rebuild(binaryPathOverride: String) {
        client = Self.makeClient(binaryPathOverride)
        generation += 1
    }

    private static func makeClient(_ override: String) -> SymBrainClient {
        SymBrainClient(userOverride: override.isEmpty ? nil : URL(fileURLWithPath: override))
    }
}

/// Existing preference keys. They predate the redesign and must not change.
enum PreferenceKey {
    static let binaryPathOverride = "binaryPathOverride"
    static let syncDryRun = "syncDryRun"
    static let updatePrefix = "com.symaira.brain"
    static let updateSkippedTag = "com.symaira.brain.updateSkippedTag"
    /// Added with the AppKit workspace; the app was dark-only before.
    static let appearance = "symbrain_appearance"
}

@main
struct SymBrainApp: App {
    @FocusedValue(\.brainDestination) private var destination
    @AppStorage(PreferenceKey.binaryPathOverride) private var binaryPathOverride = ""
    @AppStorage(PreferenceKey.appearance) private var appearance: SymairaAppearance = .system

    @StateObject private var clientHolder: ClientHolder
    @StateObject private var status = AppStatus()

    init() {
        let stored = UserDefaults.standard.string(forKey: PreferenceKey.binaryPathOverride) ?? ""
        _clientHolder = StateObject(wrappedValue: ClientHolder(binaryPathOverride: stored))
    }

    var body: some Scene {
        WindowGroup("Symaira Brain", id: PageCommands.windowID) {
            BrainWorkspace()
                .environmentObject(clientHolder)
                .environmentObject(status)
                .frame(minWidth: 960, minHeight: 600)
                .onAppear { appearance.apply() }
                .onChange(of: appearance) { _, value in value.apply() }
                .onChange(of: binaryPathOverride) { _, value in
                    clientHolder.rebuild(binaryPathOverride: value)
                }
                .task { await checkForUpdatesOnLaunch() }
        }
        .symairaWorkspaceWindow()
        // The window may not shrink below the content minimum (#308, SHELL-4).
        .windowResizability(.contentMinSize)
        .defaultSize(width: 1240, height: 800)
        .commands {
            if let destination {
                SymairaCommands(destinations: Destination.commandEntries, selection: destination)
            }
            PageCommands()
        }

        SymairaSettingsScene {
            SettingsView()
                .environmentObject(clientHolder)
                .environmentObject(status)
                .frame(width: 640)
                .frame(minHeight: 560)
        }
    }

    private func checkForUpdatesOnLaunch() async {
        let prefs = UserDefaultsAutoUpdatePreferenceStore(keyPrefix: PreferenceKey.updatePrefix)
        guard prefs.autoCheckEnabled else { return }
        let checker = AppUpdateChecker(
            checker: UpdateChecker(owner: "danieljustus", repo: "symaira-brain"),
            store: UserDefaultsSkippedVersionStore(key: PreferenceKey.updateSkippedTag),
            currentVersion: { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.0.0" },
            autoPrefs: prefs
        )
        await checker.checkOnLaunchIfEnabled()
    }
}
