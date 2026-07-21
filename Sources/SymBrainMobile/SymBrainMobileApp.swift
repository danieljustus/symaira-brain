import SwiftUI
import SymairaTheme

@main
struct SymBrainMobileApp: App {
    var body: some Scene {
        WindowGroup {
            MobileRootView()
                .preferredColorScheme(.dark)
                .tint(SymairaTheme.goldPrimary)
        }
    }
}
