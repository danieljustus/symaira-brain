import SwiftUI
import SymairaTheme
import SymBrainCore

extension CredentialsPage {
    // MARK: - Entries

    var entriesSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Search credentials", text: $vm.searchText)
                    .textFieldStyle(.symaira)
                    .onSubmit { Task { await vm.loadEntries() } }
                Button("Search") { Task { await vm.loadEntries() } }
                    .symairaButtonStyle(.secondary)
            }
            if vm.isLoading && vm.entries.isEmpty {
                SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Loading credentials…", retry: refresh) { _ in
                    EmptyView()
                }
            } else if vm.entries.isEmpty {
                SymairaPageEmpty(
                    icon: .key,
                    title: vm.searchText.isEmpty ? "No Credentials" : "No Matching Credentials",
                    message: vm.searchText.isEmpty
                        ? "The vault is empty. Use New Credential, or import files on the Import section."
                        : "Nothing matches this search. Try part of the path instead."
                )
            } else {
                HStack(alignment: .top, spacing: SymairaSpacing.medium) {
                    entryTable
                    entryDetail.frame(width: 320)
                }
            }
        }
    }

    private var selection: Binding<String?> {
        Binding(
            get: { vm.selectedPath },
            set: { path in
                guard let path, path != vm.selectedPath else { return }
                Task { await vm.select(path: path) }
            }
        )
    }

    private var entryTable: some View {
        Table(vm.entries.sorted(using: entrySort), selection: selection, sortOrder: $entrySort) {
            TableColumn("Credential", value: \.path) { entry in
                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                    Text(verbatim: entry.title).symairaText(.bodyEmphasized)
                    Text(verbatim: entry.path).symairaText(.monoSmall)
                }
            }
            TableColumn("Type") { entry in
                Text(verbatim: entry.type ?? "—").symairaText(.secondary)
            }
            .width(min: 80, ideal: 110)
        }
        .contextMenu(forSelectionType: VaultEntrySummary.ID.self) { paths in
            if let path = paths.first {
                Button("Copy Path") { Pasteboard.copy(path) }
                Divider()
                Button("Delete Credential…", role: .destructive) { deletePath = path }
            }
        }
        .onDeleteCommand {
            if let path = vm.selectedPath { deletePath = path }
        }
        .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
    }

    @ViewBuilder
    private var entryDetail: some View {
        if let detail = vm.detail {
            ScrollView {
                VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                    Text(verbatim: detail.path).symairaText(.heading).textSelection(.enabled)
                    if let modified = detail.formattedModified {
                        Text("Modified \(modified)").symairaText(.caption)
                    }
                    Text("Fields hide again 30 seconds after you reveal them.").symairaText(.caption)
                    if let totp = detail.totp {
                        SymairaFormDivider()
                        HStack(spacing: SymairaSpacing.medium) {
                            Text(verbatim: totp.code).font(SymairaTypography.readout).foregroundStyle(SymairaTokens.ink)
                            if let remaining = totp.remaining {
                                Text("\(remaining) s left").symairaText(.monoSmall)
                            }
                            Spacer()
                            SymairaIconButton("Copy One-Time Code", icon: .copy, size: 16) { vm.copyTOTP() }
                        }
                        .accessibilityElement(children: .contain)
                    }
                    SymairaFormDivider()
                    ForEach(detail.sortedFields, id: \.key) { field in
                        fieldRow(field)
                    }
                    HStack(spacing: SymairaSpacing.medium) {
                        Button("Edit…") { showEditCredential = true }
                            .symairaButtonStyle(.secondary)
                            .disabled(vm.isEditing)
                        Button("Delete…", role: .destructive) { deletePath = detail.path }
                            .symairaButtonStyle(.secondary)
                            .disabled(vm.isDeleting)
                    }
                    .padding(.top, SymairaSpacing.small)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .symairaCard()
        } else if let summary = vm.entries.first(where: { $0.path == vm.selectedPath }) {
            VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                Text(verbatim: summary.path).symairaText(.heading).textSelection(.enabled)
                Text("Secret values stay hidden until you reveal this credential.").symairaText(.secondary)
                SymairaFormDivider()
                if let type = summary.type, !type.isEmpty { DetailRow(label: "Type", value: type) }
                if let hint = summary.usageHint, !hint.isEmpty { DetailRow(label: "Usage", value: hint) }
                if let count = summary.fieldCount { DetailRow(label: "Fields", value: "\(count)") }
                if let autoRotate = summary.autoRotate { DetailRow(label: "Auto-Rotate", value: autoRotate ? "On" : "Off") }
                Button(action: { Task { await vm.revealSelectedEntry() } }) {
                    Label("Reveal Credential", symairaIcon: .visible)
                }
                .symairaButtonStyle(.secondary)
                .padding(.top, SymairaSpacing.small)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .symairaCard()
        } else {
            SymairaEmptyState(
                icon: .sidebar,
                title: "No Credential Selected",
                message: "Select a credential to see its fields. Secrets stay masked until you reveal them."
            )
            .symairaCard()
        }
    }

    private func fieldRow(_ field: (key: String, value: String, isSensitive: Bool)) -> some View {
        let revealed = vm.isRevealed(field: field.key)
        return HStack(alignment: .firstTextBaseline, spacing: SymairaSpacing.small) {
            Text(verbatim: field.key)
                .symairaText(.secondary)
                .frame(width: 100, alignment: .leading)
            Group {
                if field.isSensitive && !revealed {
                    // Masked values are not selectable, so nothing can be
                    // dragged out of a row that looks hidden.
                    Text(verbatim: String(repeating: "•", count: 12))
                        .accessibilityLabel("Hidden value")
                } else {
                    Text(verbatim: field.value).textSelection(.enabled)
                }
            }
            .symairaText(.monoSmall)
            .foregroundStyle(SymairaTokens.ink)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            if field.isSensitive {
                SymairaIconButton(revealed ? "Hide \(field.key)" : "Show \(field.key)", icon: revealed ? .hidden : .visible, size: 16) {
                    vm.toggleReveal(field: field.key)
                }
            }
            SymairaIconButton(
                field.isSensitive && !revealed ? "Show \(field.key) before copying" : "Copy \(field.key)",
                icon: .copy, size: 16
            ) {
                vm.copyField(field.key)
            }
            .disabled(field.isSensitive && !revealed)
        }
    }
}
