import SwiftUI
import SymairaTheme
import SymBrainCore

/// Creates a profile from one of the CLI's base templates.
struct NewProfileSheet: View {
    let client: SymBrainClient
    let done: (_ created: Bool) -> Void

    @State private var name = ""
    @State private var template = "personal"
    @State private var isCreating = false
    @State private var errorMessage: String?
    @FocusState private var nameFocused: Bool

    private var trimmedName: String { name.trimmingCharacters(in: .whitespaces) }

    /// Mirrors the CLI's rule so the problem shows before submitting.
    private var nameProblem: String? {
        guard !trimmedName.isEmpty else { return nil }
        guard trimmedName.range(of: "^[a-z][a-z0-9_-]*$", options: .regularExpression) != nil else {
            return "Start with a lowercase letter and use only a to z, 0 to 9, hyphens and underscores."
        }
        return nil
    }

    var body: some View {
        SymairaSheetScaffold(
            title: "New Profile",
            dismiss: { done(false) },
            primaryTitle: isCreating ? "Creating…" : "Create Profile",
            primaryDisabled: trimmedName.isEmpty || nameProblem != nil || isCreating,
            primary: { Task { await create() } }
        ) {
            SymairaFormSection(footer: "Personal gives trusted harnesses full access. Restricted is least privilege: vault by request only, memory read-only.") {
                SymairaFormRow("Name") {
                    VStack(alignment: .trailing, spacing: SymairaSpacing.xSmall) {
                        TextField("my-profile", text: $name)
                            .textFieldStyle(.symaira(isFocused: nameFocused))
                            .focused($nameFocused)
                            .frame(width: 240)
                            .onSubmit { Task { await create() } }
                        if let nameProblem {
                            Text(nameProblem)
                                .symairaText(.caption)
                                .foregroundStyle(SymairaTokens.critical)
                                .frame(width: 240, alignment: .leading)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                SymairaFormDivider()
                SymairaFormRow("Base Template") {
                    SymairaSegmentedControl(
                        "Base template", selection: $template,
                        options: [(value: "personal", label: "Personal"), (value: "restricted", label: "Restricted")]
                    )
                }
            }
            if let errorMessage {
                SymairaNotice(title: "Profile Not Created", message: errorMessage, tone: .critical)
            }
        }
        .onAppear { nameFocused = true }
    }

    private func create() async {
        guard !trimmedName.isEmpty, nameProblem == nil, !isCreating else { return }
        isCreating = true
        errorMessage = nil
        defer { isCreating = false }
        do {
            _ = try await client.profileAdd(name: trimmedName, from: template)
            done(true)
        } catch {
            errorMessage = formatError(error).message
        }
    }
}
