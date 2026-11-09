import SwiftUI
import SymairaTheme
import SymBrainCore

/// Creates one credential. Secret inputs are SecureFields and are cleared by
/// the view model after every attempt.
struct NewCredentialSheet: View {
    @ObservedObject var vm: VaultViewModel
    let close: () -> Void

    @State private var passwordLength = 24
    @State private var passwordSymbols = true

    private static let types = [
        "password", "api_key", "bearer_token", "basic_auth", "ssh_key",
        "certificate", "database_url", "totp_seed", "custom",
    ]

    private var canCreate: Bool {
        !vm.createPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !vm.createValue.isEmpty && !vm.isCreating
    }

    var body: some View {
        SymairaSheetScaffold(
            title: "New Credential",
            dismiss: close,
            primaryTitle: vm.isCreating ? "Creating…" : "Create Credential",
            primaryDisabled: !canCreate,
            primary: create
        ) {
            SymairaFormSection("Credential") {
                SymairaFormRow("Path", description: "Folders separate with a slash, for example api/openai.") {
                    TextField("service/name", text: $vm.createPath)
                        .textFieldStyle(.symaira)
                        .frame(width: 260)
                }
                SymairaFormDivider()
                SymairaFormRow("Type") {
                    SymairaFilterMenu("Type", selection: $vm.createType, options: Self.types.map { (value: $0, label: $0) })
                        .frame(width: 200)
                }
            }
            SymairaFormSection("Secret", footer: "Generate asks symvault for a random password of the chosen length.") {
                SymairaFormRow("Value") {
                    SecureField("Secret value", text: $vm.createValue)
                        .textFieldStyle(.symaira)
                        .frame(width: 260)
                }
                SymairaFormDivider()
                SymairaFormRow("Generate") {
                    HStack(spacing: SymairaSpacing.medium) {
                        Stepper("\(passwordLength) characters", value: $passwordLength, in: 12...128, step: 4)
                            .monospacedDigit()
                        Toggle("Symbols", isOn: $passwordSymbols)
                            .toggleStyle(.symairaSwitch)
                        Button(vm.isGenerating ? "Generating…" : "Generate") {
                            Task { await vm.generatePassword(length: passwordLength, symbols: passwordSymbols) }
                        }
                        .symairaButtonStyle(.secondary)
                        .disabled(vm.isGenerating)
                    }
                }
            }
            SymairaFormSection("Details", footer: "Optional. The usage hint tells agents what the credential is for.") {
                row("Username", text: $vm.createUsername, placeholder: "user@example.com")
                SymairaFormDivider()
                row("URL", text: $vm.createURL, placeholder: "https://example.com")
                SymairaFormDivider()
                row("Usage Hint", text: $vm.createUsageHint, placeholder: "Deploys to production")
                SymairaFormDivider()
                row("Notes", text: $vm.createNotes, placeholder: "None")
                SymairaFormDivider()
                row("Expires", text: $vm.createExpiresAt, placeholder: "2027-01-31T00:00:00Z")
                SymairaFormDivider()
                SymairaFormRow("Auto-Rotate") {
                    Toggle(isOn: $vm.createAutoRotate) { EmptyView() }
                        .toggleStyle(.symairaSwitch)
                        .accessibilityLabel("Auto-Rotate")
                }
            }
            SymairaFormSection("One-Time Codes", footer: "Optional. Add the TOTP secret to show codes next to this credential.") {
                SymairaFormRow("TOTP Secret") {
                    SecureField("Base32 secret", text: $vm.createTOTPSecret)
                        .textFieldStyle(.symaira)
                        .frame(width: 260)
                }
                SymairaFormDivider()
                row("Issuer", text: $vm.createTOTPIssuer, placeholder: "GitHub")
                SymairaFormDivider()
                row("Account", text: $vm.createTOTPAccount, placeholder: "user@example.com")
            }
            if let error = vm.errorMessage {
                SymairaNotice(title: "Credential Not Created", message: error, tone: .critical)
            }
        }
    }

    private func row(_ label: String, text: Binding<String>, placeholder: String) -> some View {
        SymairaFormRow(label) {
            TextField(placeholder, text: text)
                .textFieldStyle(.symaira)
                .frame(width: 260)
        }
    }

    private func create() {
        guard canCreate else { return }
        vm.createConfirmation = nil
        Task {
            await vm.createEntry()
            if vm.errorMessage == nil, vm.createConfirmation != nil { close() }
        }
    }
}

/// Updates the revealed credential. An empty value keeps the current secret.
struct EditCredentialSheet: View {
    @ObservedObject var vm: VaultViewModel
    let close: () -> Void

    var body: some View {
        SymairaSheetScaffold(
            title: "Edit \(vm.detail?.path ?? "Credential")",
            dismiss: close,
            primaryTitle: vm.isEditing ? "Saving…" : "Save Changes",
            primaryDisabled: vm.isEditing || vm.detail == nil,
            primary: save
        ) {
            if vm.detail == nil {
                SymairaNotice(
                    title: "Credential Hidden Again",
                    message: "Revealed values expire after 30 seconds. Close this sheet, reveal the credential and edit it again.",
                    tone: .warning
                )
            }
            SymairaFormSection(footer: "Leave the new value empty to keep the current secret.") {
                SymairaFormRow("New \(vm.detail?.primarySecret?.field.capitalized ?? "Value")") {
                    SecureField("Unchanged", text: $vm.editValue)
                        .textFieldStyle(.symaira)
                        .frame(width: 260)
                }
                SymairaFormDivider()
                SymairaFormRow("Username") {
                    TextField("None", text: $vm.editUsername).textFieldStyle(.symaira).frame(width: 260)
                }
                SymairaFormDivider()
                SymairaFormRow("URL") {
                    TextField("None", text: $vm.editURL).textFieldStyle(.symaira).frame(width: 260)
                }
                SymairaFormDivider()
                SymairaFormRow("Notes") {
                    TextField("None", text: $vm.editNotes).textFieldStyle(.symaira).frame(width: 260)
                }
            }
            if let error = vm.errorMessage {
                SymairaNotice(title: "Changes Not Saved", message: error, tone: .critical)
            }
        }
    }

    private func save() {
        vm.editConfirmation = nil
        Task {
            await vm.setSelectedEntry()
            if vm.errorMessage == nil, vm.editConfirmation != nil { close() }
        }
    }
}
