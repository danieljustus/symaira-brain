import SwiftUI
import SymairaTheme
import SymBrainCore

/// Dry-run preview of an install or uninstall. Nothing is written.
struct HarnessPreviewSheet: View {
    let request: HarnessPreview
    let client: SymBrainClient
    let profiles: [ProfileSummary]
    let close: () -> Void

    @State private var profile: String
    @State private var output: String?
    @State private var isRunning = false

    init(request: HarnessPreview, client: SymBrainClient, profiles: [ProfileSummary], close: @escaping () -> Void) {
        self.request = request
        self.client = client
        self.profiles = profiles
        self.close = close
        _profile = State(initialValue: profiles.first?.name ?? "")
    }

    var body: some View {
        SymairaSheetScaffold(
            title: request.isInstall ? "Preview Install in \(request.harness)" : "Preview Uninstall from \(request.harness)",
            dismissTitle: output == nil ? "Cancel" : "Close",
            dismiss: close,
            primaryTitle: request.isInstall && output == nil ? "Run Preview" : nil,
            primaryDisabled: profile.isEmpty || isRunning,
            primary: { Task { await run() } }
        ) {
            Text("A preview shows the configuration change without writing it.")
                .symairaText(.secondary)
            if request.isInstall && output == nil {
                SymairaFormSection {
                    SymairaFormRow("Profile", description: profiles.isEmpty ? "Create a profile on the Profiles page first." : nil) {
                        if !profiles.isEmpty {
                            SymairaFilterMenu("Profile", selection: $profile, options: profiles.map { (value: $0.name, label: $0.name) })
                                .frame(width: 220)
                        }
                    }
                }
            }
            if isRunning {
                SymairaLoadingState("Running preview…")
            } else if let output {
                OutputBlock(text: output)
            }
        }
        .task {
            // Uninstall needs no choice, so its preview runs right away (#74).
            if !request.isInstall { await run() }
        }
    }

    private func run() async {
        isRunning = true
        defer { isRunning = false }
        do {
            output = request.isInstall
                ? try await client.install(harness: request.harness, profile: profile, dryRun: true)
                : try await client.uninstall(harness: request.harness, dryRun: true)
        } catch {
            output = formatError(error).message
        }
    }
}
