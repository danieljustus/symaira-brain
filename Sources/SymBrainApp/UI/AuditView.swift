import SwiftUI
import SymairaTheme
import SymBrainCore

struct AuditView: View {
    let client: SymBrainClient

    @StateObject private var vm: AuditViewModel

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: AuditViewModel(client: client))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
            headerSection

            if vm.isLoading {
                SymairaLoadingState("Loading audit log...")
            } else if vm.entries.isEmpty {
                SymairaEmptyState(
                    systemImage: "doc.text.magnifyingglass",
                    title: "No Audit Entries",
                    message: "Audit entries will appear here once tool calls are routed through symbrain."
                )
            } else {
                tableSection
            }
        }
        .padding(SymairaSpacing.xLarge)
        .task {
            await vm.loadProfiles()
            await vm.refresh()
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Audit Log")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Tool call history from the JSONL audit log")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()

            Picker("Profile", selection: $vm.selectedProfile) {
                Text("All Profiles").tag(nil as String?)
                ForEach(vm.profiles) { profile in
                    Text(profile.name).tag(profile.name as String?)
                }
            }
            .pickerStyle(.menu)
            .frame(minWidth: 140)
            .onChange(of: vm.selectedProfile) { _, _ in
                Task { await vm.refresh() }
            }

            Button(action: { Task { await vm.refresh() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.secondary)
        }
    }

    // MARK: - Table

    private var tableSection: some View {
        Table(vm.entries) {
            TableColumn("Timestamp") { entry in
                Text(entry.formattedTime)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            .width(min: 140, ideal: 160)

            TableColumn("Server") { entry in
                SymairaBadge(entry.server, tone: serverTone(entry.server))
            }
            .width(min: 70, ideal: 80)

            TableColumn("Tool") { entry in
                Text(entry.tool)
                    .font(.caption.monospaced())
                    .foregroundStyle(SymairaTheme.textPrimary)
            }
            .width(min: 160, ideal: 200)

            TableColumn("Status") { entry in
                SymairaBadge(entry.status, tone: statusTone(entry.status))
            }
            .width(min: 60, ideal: 70)

            TableColumn("Duration") { entry in
                Text("\(entry.durationMs)ms")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(SymairaTheme.textMuted)
            }
            .width(min: 60, ideal: 70)
        }
    }

    private func serverTone(_ server: String) -> SymairaTone {
        switch server {
        case "vault": return .informative
        case "memory": return .positive
        case "skills": return .warning
        default: return .neutral
        }
    }

    private func statusTone(_ status: String) -> SymairaTone {
        switch status {
        case "ok": return .positive
        case "error": return .critical
        case "timeout": return .warning
        default: return .neutral
        }
    }
}
