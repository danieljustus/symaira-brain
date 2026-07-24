import SwiftUI
import SymairaTheme
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
        }
        .windowStyle(.hiddenTitleBar)
    }
}
