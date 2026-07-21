import SwiftUI
import SymairaTheme
import SymBrainCore

struct SyncView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
            Text("Sync")
                .font(.title.bold())
                .foregroundStyle(SymairaTheme.textPrimary)

            SymairaNotice(
                title: "Not Yet Implemented",
                message: "symbrain sync will push the canonical instructions/skills source out to installed harnesses. This command is planned for a future milestone.",
                tone: .informative
            )

            SymairaEmptyState(
                systemImage: "arrow.triangle.2.circlepath",
                title: "Sync Coming Soon",
                message: "The sync command will propagate profile and skill changes to all installed harnesses."
            )
        }
        .padding(SymairaSpacing.xLarge)
    }
}
