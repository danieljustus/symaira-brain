import AppKit
import SwiftUI
import SymairaTheme
import SymBrainCore

extension CredentialsPage {
    // MARK: - Approval requests

    var requestsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack {
                Text("Agents on request-only profiles ask here before they read or write a credential.")
                    .symairaText(.secondary)
                Spacer()
                Button("Reload Requests") { Task { await vm.loadApprovals() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isLoadingApprovals)
            }
            if let snapshot = vm.approvalSnapshot {
                review(snapshot.request)
            }
            if vm.isLoadingApprovals {
                SymairaPageState(state: SymairaPageLoadState<Void>.loading, loading: "Loading approval requests…", retry: {}) { _ in
                    EmptyView()
                }
            } else if vm.approvalRequests.isEmpty {
                SymairaPageEmpty(
                    icon: .shieldCheck,
                    title: "No Pending Requests",
                    message: "When an agent asks for a credential, its request waits here for your decision."
                )
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                        ForEach(vm.approvalRequests) { request in
                            requestCard(request)
                        }
                    }
                }
            }
        }
        .task { await vm.loadApprovals() }
    }

    private func requestCard(_ request: VaultApprovalRequest) -> some View {
        HStack(alignment: .top, spacing: SymairaSpacing.medium) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("\(request.agentName) wants to \(request.write ? "write" : "read") \(request.path)")
                    .symairaText(.bodyEmphasized)
                Text(verbatim: request.reason).symairaText(.secondary)
                Text("Expires \(formatModuleTimestamp(request.expiresAt)) · Request \(request.id)")
                    .symairaText(.caption)
            }
            Spacer()
            SymairaStatusLabel(
                request.isExpired ? "Expired" : request.status.capitalized,
                tone: request.isExpired ? .neutral : .warning
            )
            Button("Review…") { vm.reviewApproval(request) }
                .symairaButtonStyle(.secondary)
                .disabled(!request.isPending || request.isExpired)
        }
        .symairaCard()
        .contextMenu {
            Button("Review Request…") { vm.reviewApproval(request) }
                .disabled(!request.isPending || request.isExpired)
            Button("Copy Request ID") { Pasteboard.copy(request.id) }
        }
    }

    /// The explicit review step: the decision buttons exist only here.
    private func review(_ request: VaultApprovalRequest) -> some View {
        SymairaFormSection("Review Request", footer: "The vault checks the request again right before your decision is sent.") {
            DetailRow(label: "Agent", value: request.agentName)
            DetailRow(label: "Credential", value: request.path, monospaced: true)
            DetailRow(label: "Access", value: request.write ? "Write" : "Read")
            DetailRow(label: "Reason", value: request.reason)
            HStack(spacing: SymairaSpacing.medium) {
                Spacer()
                Button("Cancel") { vm.cancelApprovalReview() }
                    .symairaButtonStyle(.secondary)
                    .keyboardShortcut(.cancelAction)
                Button("Deny Request", role: .destructive) { Task { await vm.decideReviewedApproval(approve: false) } }
                    .symairaButtonStyle(.secondary)
                Button(request.write ? "Allow Write" : "Allow Read") {
                    Task { await vm.decideReviewedApproval(approve: true) }
                }
                .symairaButtonStyle(.primary)
            }
            .disabled(vm.isDecidingApproval)
            .padding(.vertical, SymairaSpacing.small)
        }
    }

    // MARK: - Import

    var importSection: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                SymairaFormSection(
                    "Files",
                    footer: "Import reads credential files, suggests vault paths and fields, and writes nothing until you import the reviewed batch."
                ) {
                    SymairaFormRow(
                        vm.intakeFiles.isEmpty ? "No Files Chosen" : "\(vm.intakeFiles.count) Files",
                        description: vm.intakeFiles.isEmpty ? nil : vm.intakeFiles.map(\.lastPathComponent).joined(separator: ", ")
                    ) {
                        HStack(spacing: SymairaSpacing.medium) {
                            if !vm.intakeFiles.isEmpty {
                                Button("Clear") { vm.clearIntake() }.symairaButtonStyle(.secondary)
                            }
                            Button("Choose Files…", action: chooseFiles).symairaButtonStyle(.secondary)
                        }
                    }
                    SymairaFormDivider()
                    SymairaFormRow("Move Originals to Trash", description: "After staging, the source files go to the Trash.") {
                        Toggle(isOn: $vm.intakeMoveToTrash) { EmptyView() }
                            .toggleStyle(.symairaSwitch)
                            .accessibilityLabel("Move Originals to Trash")
                    }
                }
                HStack(spacing: SymairaSpacing.medium) {
                    Button("Preview Import") { Task { await vm.previewIntake() } }
                        .symairaButtonStyle(.secondary)
                        .disabled(vm.intakeFiles.isEmpty || isIntakeBusy)
                    Button("Stage for Review") { Task { await vm.stageIntake() } }
                        .symairaButtonStyle(.secondary)
                        .disabled(vm.intakeFiles.isEmpty || isIntakeBusy)
                    Spacer()
                    if isIntakeBusy {
                        Text(intakeProgress).symairaText(.secondary)
                    }
                }
                if case .failed(let message) = vm.intakePhase {
                    SymairaNotice(title: "Import Failed", message: message, tone: .critical)
                }
                if !vm.intakeDrafts.isEmpty {
                    drafts
                }
            }
        }
    }

    private var drafts: some View {
        SymairaFormSection(
            vm.intakeImportID == nil ? "Preview" : "Staged Batch",
            footer: vm.intakeImportID == nil ? "Stage the files to review and import them." : nil
        ) {
            ForEach(Array(vm.intakeDrafts.enumerated()), id: \.element.id) { index, draft in
                if index > 0 { SymairaFormDivider() }
                SymairaFormRow(draft.sourceName, description: "→ \(draft.targetPath) · \(draft.fields.keys.sorted().joined(separator: ", "))")
                ForEach(draft.warnings, id: \.self) { warning in
                    Text(warning).symairaText(.caption).foregroundStyle(SymairaTokens.warning)
                }
            }
            if vm.intakeImportID != nil {
                HStack(spacing: SymairaSpacing.medium) {
                    Spacer()
                    Button(vm.intakeReviewComplete ? "Reviewed" : "Mark as Reviewed") { vm.markIntakeReviewed() }
                        .symairaButtonStyle(.secondary)
                        .disabled(vm.intakeReviewComplete)
                    Button("Import Reviewed Batch") { Task { await vm.promoteIntake() } }
                        .symairaButtonStyle(.primary)
                        .disabled(!vm.intakeReviewComplete || isIntakeBusy)
                }
                .padding(.vertical, SymairaSpacing.small)
                if !vm.intakeReviewComplete {
                    Text("Mark the batch as reviewed before importing it.").symairaText(.caption)
                }
            }
        }
    }

    private var isIntakeBusy: Bool {
        switch vm.intakePhase {
        case .previewing, .staging, .promoting: true
        default: false
        }
    }

    private var intakeProgress: String {
        switch vm.intakePhase {
        case .previewing: "Reading files…"
        case .staging: "Staging files…"
        case .promoting: "Importing…"
        default: ""
        }
    }

    private func chooseFiles() {
        let panel = NSOpenPanel()
        panel.title = "Choose Credential Files"
        panel.prompt = "Choose"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        vm.setIntakeFiles(vm.intakeFiles + panel.urls)
    }
}
