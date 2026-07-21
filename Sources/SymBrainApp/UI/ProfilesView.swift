import SwiftUI
import SymairaTheme
import SymBrainCore

struct ProfilesView: View {
    let client: SymBrainClient

    @StateObject private var vm: ProfilesViewModel
    @State private var showNewProfileSheet = false
    @State private var showDeleteConfirmation = false
    @State private var profileToDelete: String?

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: ProfilesViewModel(client: client))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: SymairaSpacing.xLarge) {
                headerSection

                if vm.isLoading && vm.profiles.isEmpty {
                    SymairaLoadingState("Loading profiles...")
                } else if let error = vm.errorMessage {
                    SymairaNotice(title: "Error", message: error, tone: .critical)
                } else {
                    profileListSection
                }
            }
            .padding(SymairaSpacing.xLarge)
        }
        .task {
            await vm.loadProfiles()
        }
        .sheet(isPresented: $showNewProfileSheet) {
            NewProfileSheet(client: client) {
                showNewProfileSheet = false
                Task { await vm.loadProfiles() }
            }
        }
        .alert("Delete Profile", isPresented: $showDeleteConfirmation) {
            Button("Cancel", role: .cancel) { profileToDelete = nil }
            Button("Delete", role: .destructive) {
                if let name = profileToDelete {
                    Task { _ = await vm.removeProfile(name: name) }
                }
                profileToDelete = nil
            }
        } message: {
            if let name = profileToDelete {
                Text("Are you sure you want to delete profile \"\(name)\"? This cannot be undone.")
            }
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text("Profiles")
                    .font(.title.bold())
                    .foregroundStyle(SymairaTheme.textPrimary)
                Text("Manage MCP exposure profiles for harness connections")
                    .font(.subheadline)
                    .foregroundStyle(SymairaTheme.textSecondary)
            }
            Spacer()
            Button(action: { showNewProfileSheet = true }) {
                Label("New Profile", systemImage: "plus")
            }
            .symairaButtonStyle(.primary)
        }
    }

    // MARK: - Profile List

    private var profileListSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            ForEach(vm.profiles) { profile in
                profileRow(profile)
            }
        }
    }

    @ViewBuilder
    private func profileRow(_ profile: ProfileSummary) -> some View {
        Button(action: {
            Task { await vm.selectProfile(profile.name) }
        }) {
            HStack {
                VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                    Text(profile.name)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(SymairaTheme.textPrimary)
                    Text(profile.description)
                        .font(.caption)
                        .foregroundStyle(SymairaTheme.textSecondary)
                }
                Spacer()
                ForEach(profile.servers, id: \.server) { server in
                    SymairaBadge(
                        "\(server.server): \(server.mode ?? (server.enabled ? "on" : "off"))",
                        tone: server.enabled ? .positive : .neutral
                    )
                }
                Button(action: {
                    profileToDelete = profile.name
                    showDeleteConfirmation = true
                }) {
                    Image(systemName: "trash")
                        .foregroundStyle(SymairaTheme.critical)
                }
                .buttonStyle(.plain)
            }
            .padding(SymairaSpacing.medium)
            .glassCard()
        }
        .buttonStyle(.plain)

        // Detail pane inline
        if vm.selectedProfile?.name == profile.name, let detail = vm.selectedProfile {
            profileDetail(detail)
        }
    }

    private func profileDetail(_ detail: ProfileDetail) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            ForEach(detail.servers, id: \.server) { server in
                VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                    HStack {
                        Text(server.server)
                            .font(.body.weight(.semibold))
                            .foregroundStyle(SymairaTheme.textPrimary)
                        SymairaBadge(server.enabled ? "Enabled" : "Disabled", tone: server.enabled ? .positive : .neutral)
                        if let mode = server.mode {
                            SymairaBadge(mode, tone: .informative)
                        }
                    }

                    if let policy = server.effectivePolicy {
                        if !policy.exposed.isEmpty {
                            Text("Exposed")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(SymairaTheme.positive)
                            FlowLayout(spacing: SymairaSpacing.xSmall) {
                                ForEach(policy.exposed, id: \.self) { tool in
                                    SymairaBadge(tool, tone: .positive)
                                }
                            }
                        }
                        if !policy.hidden.isEmpty {
                            Text("Hidden")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(SymairaTheme.critical)
                            FlowLayout(spacing: SymairaSpacing.xSmall) {
                                ForEach(policy.hidden, id: \.self) { tool in
                                    SymairaBadge(tool, tone: .critical)
                                }
                            }
                        }
                    }

                    if let note = server.note {
                        Text(note)
                            .font(.caption)
                            .foregroundStyle(SymairaTheme.textMuted)
                            .italic()
                    }
                }
                .padding(SymairaSpacing.medium)
                .glassCard()
            }
        }
        .padding(.leading, SymairaSpacing.xLarge)
    }
}

// MARK: - New Profile Sheet

struct NewProfileSheet: View {
    let client: SymBrainClient
    let onDone: () -> Void

    @State private var name = ""
    @State private var template = "personal"
    @State private var isCreating = false
    @State private var errorMessage: String?

    private let templates = ["personal", "restricted"]

    var body: some View {
        VStack(spacing: SymairaSpacing.xLarge) {
            Text("New Profile")
                .font(.title2.bold())
                .foregroundStyle(SymairaTheme.textPrimary)

            VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                Text("Name")
                    .font(.headline)
                    .foregroundStyle(SymairaTheme.textSecondary)
                TextField("my-profile", text: $name)
                    .textFieldStyle(.roundedBorder)

                Text("Base template")
                    .font(.headline)
                    .foregroundStyle(SymairaTheme.textSecondary)
                Picker("Template", selection: $template) {
                    ForEach(templates, id: \.self) { t in
                        Text(t).tag(t)
                    }
                }
                .pickerStyle(.segmented)
            }

            if let error = errorMessage {
                SymairaNotice(title: "Error", message: error, tone: .critical)
            }

            HStack {
                Button("Cancel") { onDone() }
                    .symairaButtonStyle(.secondary)
                Spacer()
                Button("Create") {
                    isCreating = true
                    Task {
                        let ok = await addProfile()
                        isCreating = false
                        if ok { onDone() }
                    }
                }
                .symairaButtonStyle(.primary)
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || isCreating)
            }
        }
        .padding(SymairaSpacing.xLarge)
        .frame(width: 420)
    }

    private func addProfile() async -> Bool {
        errorMessage = nil
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        do {
            _ = try await client.profileAdd(name: trimmed, from: template)
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }
}

// MARK: - FlowLayout

/// Simple horizontal flow layout for badges.
private struct FlowLayout: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let result = arrange(proposal: proposal, subviews: subviews)
        return result.size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(proposal: proposal, subviews: subviews)
        for (index, origin) in result.origins.enumerated() where index < subviews.count {
            subviews[index].place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(proposal: ProposedViewSize, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        let maxWidth = proposal.width ?? .infinity
        var origins: [CGPoint] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var totalWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > maxWidth, x > 0 {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            rowHeight = max(rowHeight, size.height)
            x += size.width + spacing
            totalWidth = max(totalWidth, x)
        }

        return (CGSize(width: totalWidth, height: y + rowHeight), origins)
    }
}
