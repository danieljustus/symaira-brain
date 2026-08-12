import SwiftUI
import SymairaTheme
import SymBrainCore

/// The embedded Symaira Vault module: unlock the local vault, browse and
/// search entries, reveal or copy single fields, and read the symbrain broker
/// audit trail for the vault server.
///
/// Secrets are fetched only for the selected entry and stay masked until the
/// user reveals them explicitly.
struct VaultView: View {
    @StateObject private var vm = VaultViewModel()
    @State private var tab: Tab = .entries

    enum Tab: String, CaseIterable, Identifiable {
        case entries = "Entries"
        case activity = "Broker Activity"

        var id: String { rawValue }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            headerSection

            switch vm.availability {
            case .checking:
                SymairaLoadingState("Connecting to the vault…")

            case .missing:
                missingRuntimeSection

            case .failed(let message):
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    SymairaNotice(title: "Vault unavailable", message: message, tone: .critical)
                    Button(action: { Task { await vm.refresh() } }) {
                        Label("Try Again", systemImage: "arrow.clockwise")
                    }
                    .symairaButtonStyle(.primary)
                }

            case .locked:
                unlockSection

            case .ready:
                Picker("Section", selection: $tab) {
                    ForEach(Tab.allCases) { item in
                        Text(item.rawValue).tag(item)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                if let error = vm.errorMessage {
                    SymairaNotice(title: "Error", message: error, tone: .critical)
                }

                switch tab {
                case .entries: entriesSection
                case .activity: activitySection
                }
            }
        }
        .padding(SymairaSpacing.xLarge)
        .task {
            await vm.refresh()
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Vault")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Symaira Vault — credentials, secrets and their broker trail")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()

            if let version = vm.versionLine, !version.isEmpty {
                SymairaBadge(version, tone: .informative)
            }
            SymairaBadge(availabilityLabel, tone: availabilityTone)
            if let status = vm.statusMessage {
                SymairaBadge(status, tone: .positive)
            }

            if vm.isReady {
                Button(action: { Task { await vm.lock() } }) {
                    Label("Lock", systemImage: "lock.fill")
                }
                .symairaButtonStyle(.secondary)
            }

            Button(action: { Task { await vm.refresh() } }) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .symairaButtonStyle(.secondary)
        }
    }

    private var availabilityLabel: String {
        switch vm.availability {
        case .checking: "checking"
        case .missing: "not installed"
        case .locked: "locked"
        case .ready: "unlocked"
        case .failed: "error"
        }
    }

    private var availabilityTone: SymairaTone {
        switch vm.availability {
        case .ready: .positive
        case .locked: .warning
        case .missing, .failed: .critical
        case .checking: .neutral
        }
    }

    // MARK: - Runtime missing

    private var missingRuntimeSection: some View {
        SymairaEmptyState(
            systemImage: "lock.square.stack",
            title: "symvault not found",
            message: "Install the Symaira Vault runtime, then reload this screen."
        ) {
            VStack(spacing: SymairaSpacing.medium) {
                HStack(spacing: SymairaSpacing.small) {
                    Text(vm.homebrewCommand)
                        .font(.callout.monospaced())
                        .foregroundStyle(SymairaTheme.goldPrimary)
                        .textSelection(.enabled)
                    Button {
                        vm.copyToPasteboard(vm.homebrewCommand, label: "Install command")
                    } label: {
                        Image(systemName: "doc.on.doc")
                    }
                    .buttonStyle(.plain)
                    .help("Copy command")
                }
                Button(action: { Task { await vm.refresh() } }) {
                    Label("Check Again", systemImage: "arrow.clockwise")
                }
                .symairaButtonStyle(.primary)
            }
        }
    }

    // MARK: - Unlock

    private var unlockSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            SymairaNotice(
                title: "Vault locked",
                message: "Unlock the vault to browse entries. The passphrase is handed to "
                    + "symvault over stdin and never stored by SymBrain.",
                tone: .informative
            )

            HStack(spacing: SymairaSpacing.medium) {
                SecureField("Vault passphrase", text: $vm.passphrase)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 320)
                    .onSubmit { Task { await vm.unlock() } }

                Picker("Session", selection: $vm.sessionTTL) {
                    Text("5 minutes").tag("5m")
                    Text("15 minutes").tag("15m")
                    Text("1 hour").tag("1h")
                    Text("8 hours").tag("8h")
                }
                .pickerStyle(.menu)
                .frame(width: 180)

                Button(action: { Task { await vm.unlock() } }) {
                    Label("Unlock", systemImage: "lock.open")
                }
                .symairaButtonStyle(.primary)
                .disabled(vm.isUnlocking || vm.passphrase.isEmpty)
            }

            if vm.isUnlocking {
                SymairaLoadingState("Unlocking…")
            }

            if let error = vm.errorMessage {
                VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                    SymairaNotice(title: "Unlock failed", message: error, tone: .critical)
                    if let detail = vm.errorDetail {
                        SymairaNotice(title: "Details", message: detail, tone: .neutral)
                    }
                }
            }
        }
    }

    // MARK: - Entries

    private var entriesSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search entries…", text: $vm.searchText)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { Task { await vm.loadEntries() } }
                Button(action: { Task { await vm.loadEntries() } }) {
                    Label("Search", systemImage: "magnifyingglass")
                }
                .symairaButtonStyle(.secondary)
            }

            if vm.isLoading && vm.entries.isEmpty {
                SymairaLoadingState("Loading entries…")
            } else if vm.entries.isEmpty {
                SymairaEmptyState(
                    systemImage: "key",
                    title: "No Entries",
                    message: "This vault has no entries matching the current filter."
                )
            } else {
                HSplitView {
                    entryList
                    entryDetail
                }
            }
        }
    }

    private var entryList: some View {
        List {
            ForEach(vm.groupedEntries, id: \.group) { section in
                Section(section.group) {
                    ForEach(section.entries) { entry in
                        Button(action: { Task { await vm.select(path: entry.path) } }) {
                            HStack {
                                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                                    Text(entry.title)
                                        .font(.callout.weight(.medium))
                                        .foregroundStyle(SymairaTheme.textPrimary)
                                    Text(entry.path)
                                        .font(.caption.monospaced())
                                        .foregroundStyle(SymairaTheme.textMuted)
                                }
                                Spacer()
                                if let type = entry.type, !type.isEmpty {
                                    SymairaBadge(type, tone: .neutral)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(
                            vm.selectedPath == entry.path
                                ? SymairaTheme.bgCardHover
                                : Color.clear
                        )
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .frame(minWidth: 280, idealWidth: 340)
    }

    @ViewBuilder
    private var entryDetail: some View {
        if let detail = vm.detail {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                    Text(detail.path)
                        .font(.title3.bold())
                        .foregroundStyle(SymairaTheme.textPrimary)
                        .textSelection(.enabled)

                    if let modified = detail.formattedModified {
                        Text("Modified \(modified)")
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                    }

                    if let totp = detail.totp {
                        HStack(spacing: SymairaSpacing.small) {
                            SymairaBadge("TOTP \(totp.code)", tone: .informative)
                            if let remaining = totp.remaining {
                                Text("\(remaining)s left")
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(SymairaTheme.textMuted)
                            }
                            Button("Copy") {
                                vm.copyToPasteboard(totp.code, label: "TOTP code", concealed: true)
                            }
                            .symairaButtonStyle(.secondary)
                        }
                    }

                    ForEach(detail.sortedFields, id: \.key) { field in
                        fieldRow(field)
                    }
                }
                .padding(SymairaSpacing.large)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minWidth: 340)
        } else if vm.selectedPath != nil {
            SymairaLoadingState("Loading entry…")
                .frame(minWidth: 340)
        } else {
            SymairaEmptyState(
                systemImage: "sidebar.right",
                title: "No Entry Selected",
                message: "Select an entry to view its fields. Secrets stay masked until revealed."
            )
            .frame(minWidth: 340)
        }
    }

    private func fieldRow(
        _ field: (key: String, value: String, isSensitive: Bool)
    ) -> some View {
        let revealed = vm.isRevealed(field: field.key)
        return HStack(alignment: .top, spacing: SymairaSpacing.small) {
            Text(field.key)
                .font(.caption.weight(.semibold))
                .foregroundStyle(SymairaTheme.textSecondary)
                .frame(width: 120, alignment: .leading)

            Group {
                if field.isSensitive && !revealed {
                    // Masked values are not selectable — otherwise the secret
                    // could be dragged out of a row that looks hidden.
                    Text(String(repeating: "•", count: 12))
                } else {
                    Text(field.value).textSelection(.enabled)
                }
            }
            .font(.caption.monospaced())
            .foregroundStyle(SymairaTheme.textPrimary)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)

            if field.isSensitive {
                Button {
                    vm.toggleReveal(field: field.key)
                } label: {
                    Image(systemName: revealed ? "eye.slash" : "eye")
                }
                .buttonStyle(.plain)
                .help(revealed ? "Hide value" : "Reveal value")
            }

            Button {
                vm.copyToPasteboard(
                    field.value,
                    label: field.key,
                    concealed: field.isSensitive
                )
            } label: {
                Image(systemName: "doc.on.doc")
            }
            .buttonStyle(.plain)
            .help("Copy value")
        }
        .padding(.vertical, SymairaSpacing.xSmall)
    }

    // MARK: - Broker activity

    private var activitySection: some View {
        ModuleActivityTable(
            entries: vm.brokerActivity,
            emptyMessage: "Calls routed to the vault server through symbrain appear here.",
            onRefresh: { vm.loadActivity() }
        )
    }
}
