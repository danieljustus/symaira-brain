import SwiftUI
import SymairaTheme
import SymairaUpdateCheck
import SymBrainCore

/// Observable holder for the SymBrainClient so it can be rebuilt at runtime
/// when the binary path override changes, instead of requiring a full restart.
final class ClientHolder: ObservableObject {
    @Published var client: SymBrainClient

    init(binaryPathOverride: String) {
        let override: URL? = binaryPathOverride.isEmpty
            ? nil
            : URL(fileURLWithPath: binaryPathOverride)
        self.client = SymBrainClient(userOverride: override)
    }

    func rebuild(binaryPathOverride: String) {
        let override: URL? = binaryPathOverride.isEmpty
            ? nil
            : URL(fileURLWithPath: binaryPathOverride)
        client = SymBrainClient(userOverride: override)
    }
}

@main
struct SymBrainApp: App {
    @AppStorage("binaryPathOverride") private var binaryPathOverride = ""

    @StateObject private var clientHolder: ClientHolder

    init() {
        let stored = UserDefaults.standard.string(forKey: "binaryPathOverride") ?? ""
        _clientHolder = StateObject(wrappedValue: ClientHolder(binaryPathOverride: stored))
    }

    var body: some Scene {
        WindowGroup {
            ContentView(client: clientHolder.client)
                .preferredColorScheme(.dark)
                .tint(SymairaTheme.goldPrimary)
                .background(SymairaTheme.bgDark)
                .environmentObject(clientHolder)
                .onChange(of: binaryPathOverride) { _, newValue in
                    clientHolder.rebuild(binaryPathOverride: newValue)
                }
                .task {
                    await checkForUpdatesOnLaunch()
                }
        }
        .windowStyle(.hiddenTitleBar)
    }

    // MARK: - Launch auto-update check

    private func checkForUpdatesOnLaunch() async {
        let prefs = UserDefaultsAutoUpdatePreferenceStore(keyPrefix: "com.symaira.brain")
        guard prefs.autoCheckEnabled else { return }
        let checker = AppUpdateChecker(
            checker: UpdateChecker(owner: "danieljustus", repo: "symaira-brain"),
            store: UserDefaultsSkippedVersionStore(key: "com.symaira.brain.updateSkippedTag"),
            currentVersion: { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.0.0" },
            autoPrefs: prefs
        )
        await checker.checkOnLaunchIfEnabled()
    }
}
