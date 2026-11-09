import SwiftUI
import SymairaTheme
import SymBrainCore

/// Credentials: the secrets in Symaira Vault, masked until revealed.
///
/// Screen card — Destination: Credentials (6 of 7). Job: find, reveal,
/// create and maintain credentials; primary action New Credential (Entries).
/// Data: the separate `symvault` process (status, list/find, entry, set,
/// delete, approvals, intake), loaded on open and ⌘R; revealed fields expire
/// after 30 seconds. States: connecting, symvault not installed, locked,
/// error with Retry, no entries / no match. Density: sortable table with a
/// detail column. Platform: macOS workspace. Broker calls moved to Activity.
struct CredentialsPage: View {
    enum Section: String, CaseIterable, Hashable {
        case entries, requests, importing

        var title: String {
            switch self {
            case .entries: "Entries"
            case .requests: "Requests"
            case .importing: "Import"
            }
        }
    }

    @StateObject var vm = VaultViewModel()
    @State var section: Section = .entries
    @State var showNewCredential = false
    @State var showEditCredential = false
    @State var deletePath: String?
    @State var entrySort = [KeyPathComparator(\VaultEntrySummary.path)]
    @FocusState private var passphraseFocused: Bool

    var body: some View {
        SymairaPage(
            title: "Credentials",
            subtitle: "Secrets in Symaira Vault, masked until you reveal them",
            scrolls: false,
            maxWidth: PageWidth.table
        ) {
            if vm.isReady {
                SymairaSegmentedControl(
                    "Credentials section", selection: $section,
                    options: Section.allCases.map { (value: $0, label: $0.title) }
                )
            }
            Spacer()
            if vm.isReady {
                Button(action: { Task { await vm.lock() } }) { Label("Lock Vault", symairaIcon: .lock) }
                    .symairaButtonStyle(.secondary)
            }
            SymairaIconButton("Refresh", icon: .refresh, action: refresh)
                .disabled(vm.isLoading)
            if vm.isReady && section == .entries {
                Button(action: { showNewCredential = true }) { Label("New Credential", symairaIcon: .add) }
                    .symairaButtonStyle(.primary)
            }
        } content: {
            content
        }
        .task { await vm.refresh() }
        .pageRefresh(refresh)
        .pagePrimaryAction(vm.isReady && section == .entries ? PageAction("New Credential") { showNewCredential = true } : nil)
        .sheet(isPresented: $showNewCredential) {
            NewCredentialSheet(vm: vm) { showNewCredential = false }
                .frame(width: 620, height: 680)
        }
        .sheet(isPresented: $showEditCredential) {
            EditCredentialSheet(vm: vm) { showEditCredential = false }
                .frame(width: 560, height: 520)
        }
        .confirmationDialog(
            "Delete \(deletePath ?? "")?",
            isPresented: Binding(get: { deletePath != nil }, set: { if !$0 { deletePath = nil } }),
            presenting: deletePath
        ) { path in
            Button("Delete Credential", role: .destructive) {
                Task { await vm.deleteEntry(path: path) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("The credential is removed from the vault, and agents can no longer request it.")
        }
    }

    func refresh() {
        Task { await vm.refresh() }
    }

    @ViewBuilder
    private var content: some View {
        switch vm.availability {
        case .checking:
            SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Connecting to the vault…", retry: refresh) { _ in
                EmptyView()
            }
        case .missing:
            SymairaPageEmpty(
                icon: .productVault,
                title: "Symaira Vault Not Installed",
                message: "Credentials live in the separate symvault service. Install it, then refresh this page."
            ) {
                CommandLineView(vm.homebrewCommand).frame(maxWidth: 420)
            }
        case .failed(let message):
            SymairaPageState(state: SymairaPageLoadState<Void>.failed(message), loading: "", retry: refresh) { _ in
                EmptyView()
            }
        case .locked:
            unlock
        case .ready:
            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                notices
                switch section {
                case .entries: entriesSection
                case .requests: requestsSection
                case .importing: importSection
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)
        }
    }

    @ViewBuilder
    private var notices: some View {
        if let error = vm.errorMessage {
            SymairaNotice(
                message: [error, vm.errorDetail].compactMap { $0 }.joined(separator: "\n"),
                tone: .critical
            ) { vm.clearError() }
        } else if let status = vm.statusMessage {
            SymairaNotice(message: status, tone: .positive) { vm.statusMessage = nil }
        }
    }

    // MARK: - Locked

    private var unlock: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            SymairaFormSection(
                "Unlock Vault",
                footer: "The passphrase goes to symvault over standard input. Symaira Brain never stores it."
            ) {
                SymairaFormRow("Passphrase") {
                    SecureField("Vault passphrase", text: $vm.passphrase)
                        .textFieldStyle(.symaira(isFocused: passphraseFocused))
                        .focused($passphraseFocused)
                        .frame(width: 260)
                        .onSubmit { Task { await vm.unlock() } }
                }
                SymairaFormDivider()
                SymairaFormRow("Stay Unlocked For") {
                    SymairaSegmentedControl(
                        "Session length", selection: $vm.sessionTTL,
                        options: [
                            (value: "5m", label: "5 min"), (value: "15m", label: "15 min"),
                            (value: "1h", label: "1 hour"), (value: "8h", label: "8 hours"),
                        ]
                    )
                }
            }
            HStack(spacing: SymairaSpacing.medium) {
                Button(action: { Task { await vm.unlock() } }) {
                    Label(vm.isUnlocking ? "Unlocking…" : "Unlock Vault", symairaIcon: .unlock)
                }
                .symairaButtonStyle(.primary)
                .disabled(vm.isUnlocking || vm.passphrase.isEmpty)
                if vm.passphrase.isEmpty {
                    Text("Enter the passphrase to unlock.").symairaText(.caption)
                }
            }
            if let error = vm.errorMessage {
                SymairaNotice(
                    title: "Vault Still Locked",
                    message: [error, vm.errorDetail].compactMap { $0 }.joined(separator: "\n"),
                    tone: .critical
                )
            }
        }
        .frame(maxWidth: SymairaMetrics.readableContentWidth, alignment: .leading)
        .onAppear { passphraseFocused = true }
    }
}
