import SwiftUI
import SymairaTheme
import SymBrainCore

@main
struct SymBrainApp: App {
    @AppStorage("binaryPathOverride") private var binaryPathOverride = ""

    private var client: SymBrainClient {
        let override: URL? = binaryPathOverride.isEmpty
            ? nil
            : URL(fileURLWithPath: binaryPathOverride)
        return SymBrainClient(userOverride: override)
    }

    var body: some Scene {
        WindowGroup {
            ContentView(client: client)
                .preferredColorScheme(.dark)
                .tint(SymairaTheme.goldPrimary)
                .background(SymairaTheme.bgDark)
        }
        .windowStyle(.hiddenTitleBar)
    }
}
