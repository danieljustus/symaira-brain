import SwiftUI
import SymairaTheme

@main
struct SymBrainMobileApp: App {
    var body: some Scene {
        WindowGroup {
            // Follows the system appearance; the app was dark-only before.
            MobileRootView()
                .tint(SymairaTokens.selected)
        }
    }
}
