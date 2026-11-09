import SwiftUI
import SymairaTheme
import SymBrainCore

/// Profiles: decide which state cores and tools each harness connection sees.
///
/// Screen card — Destination: Profiles (3 of 7). Job: create or inspect an
/// exposure profile; primary action New Profile. Data: `symbrain profile
/// list` and `profile show` (on open, on selection, ⌘R). States: loading
/// profiles, no profiles, CLI error with Retry. Density: prose column of
/// cards. Platform: macOS workspace.
struct ProfilesPage: View {
    let client: SymBrainClient

    @StateObject private var vm: ProfilesViewModel
    @State private var hasLoaded = false
    @State private var showNewProfile = false
    @State private var profileToDelete: String?

    init(client: SymBrainClient) {
        self.client = client
        _vm = StateObject(wrappedValue: ProfilesViewModel(client: client))
    }

    var body: some View {
        SymairaPage(title: "Profiles", subtitle: "Decide which state cores and tools each harness connection can see") {
            Spacer()
            Button(action: refresh) { Label("Refresh", symairaIcon: .refresh) }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isLoading)
            Button(action: { showNewProfile = true }) { Label("New Profile", symairaIcon: .add) }
                .symairaButtonStyle(.primary)
        } content: {
            SymairaPageState(
                state: PageLoad.state(hasLoaded: hasLoaded, error: vm.errorMessage, value: vm.profiles),
                loading: "Loading profiles…", retry: refresh
            ) { profiles in
                if profiles.isEmpty {
                    SymairaPageEmpty(
                        icon: .shieldCheck,
                        title: "No Profiles",
                        message: "A profile decides which state cores a harness can reach. Use New Profile to create one."
                    )
                } else {
                    VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
                        ForEach(profiles) { profile in
                            ProfileCard(
                                profile: profile,
                                detail: vm.selectedProfile?.name == profile.name ? vm.selectedProfile : nil,
                                toggle: { Task { await vm.selectProfile(profile.name) } },
                                delete: { profileToDelete = profile.name }
                            )
                        }
                    }
                }
            }
        }
        .task {
            await vm.loadProfiles()
            hasLoaded = true
        }
        .pageRefresh(refresh)
        .pagePrimaryAction(PageAction("New Profile") { showNewProfile = true })
        .sheet(isPresented: $showNewProfile) {
            NewProfileSheet(client: client) { created in
                showNewProfile = false
                if created { refresh() }
            }
            .frame(width: 520, height: 420)
        }
        .confirmationDialog(
            "Delete Profile \(profileToDelete ?? "")?",
            isPresented: Binding(get: { profileToDelete != nil }, set: { if !$0 { profileToDelete = nil } }),
            presenting: profileToDelete
        ) { name in
            Button("Delete Profile", role: .destructive) {
                Task { _ = await vm.removeProfile(name: name) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Harnesses installed with this profile lose their access until you install another one.")
        }
    }

    private func refresh() {
        Task { await vm.loadProfiles() }
    }
}

/// One profile: summary line, per-server exposure, and on demand the tools
/// each server exposes or hides.
private struct ProfileCard: View {
    let profile: ProfileSummary
    let detail: ProfileDetail?
    let toggle: () -> Void
    let delete: () -> Void

    @State private var isHovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack(alignment: .top, spacing: SymairaSpacing.medium) {
                // Expand and delete are sibling buttons: a button nested in
                // another button's label loses its own focus and name.
                Button(action: toggle) {
                    HStack(alignment: .top, spacing: SymairaSpacing.medium) {
                        SymairaIconView(detail == nil ? .expand : .collapse, size: 16)
                            .foregroundStyle(SymairaTokens.ink3)
                            .padding(.top, 2)
                        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                            Text(verbatim: profile.name).symairaText(.subheading)
                            if let error = profile.error {
                                Text(error).symairaText(.caption).foregroundStyle(SymairaTokens.critical)
                            } else {
                                Text(verbatim: profile.description).symairaText(.secondary)
                            }
                            FlowLayout {
                                ForEach(profile.servers, id: \.server) { server in
                                    SymairaBadge(
                                        "\(server.server) \(server.mode ?? (server.enabled ? "on" : "off"))",
                                        tone: server.enabled ? .positive : .neutral
                                    )
                                }
                            }
                        }
                        Spacer(minLength: 0)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(accessibilityLabel)
                .accessibilityHint(detail == nil ? "Shows the tools this profile exposes" : "Hides the tool detail")

                SymairaIconButton("Delete Profile \(profile.name)", icon: .delete, size: 18, action: delete)
            }

            if let detail {
                ForEach(detail.servers, id: \.server) { server in
                    SymairaFormDivider()
                    serverDetail(server)
                }
                ForEach(detail.warnings ?? [], id: \.self) { warning in
                    SymairaNotice(message: warning, tone: .warning)
                }
            }
        }
        .symairaCard()
        .overlay(
            RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous)
                .stroke(SymairaTokens.line, lineWidth: isHovered ? 1 : 0)
        )
        .onHover { isHovered = $0 }
        .contextMenu {
            Button(detail == nil ? "Show Tools" : "Hide Tools", action: toggle)
            Button("Copy Name") { Pasteboard.copy(profile.name) }
            Divider()
            Button("Delete Profile…", role: .destructive, action: delete)
        }
    }

    private func serverDetail(_ server: ProfileServerDetail) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack(spacing: SymairaSpacing.medium) {
                Text(verbatim: server.server).symairaText(.bodyEmphasized)
                SymairaStatusLabel(server.enabled ? "Enabled" : "Disabled", tone: server.enabled ? .positive : .neutral)
                if let mode = server.mode {
                    SymairaBadge(mode, tone: .informative)
                }
            }
            if let policy = server.effectivePolicy {
                toolGroup("Exposed", tools: policy.exposed, tone: .positive)
                toolGroup("Hidden", tools: policy.hidden, tone: .neutral)
            }
            if let note = server.note {
                Text(note).symairaText(.caption)
            }
        }
    }

    @ViewBuilder
    private func toolGroup(_ title: String, tools: [String], tone: SymairaTone) -> some View {
        if !tools.isEmpty {
            Text(title.uppercased()).symairaText(.sectionLabel)
            FlowLayout {
                ForEach(tools, id: \.self) { tool in
                    SymairaBadge(tool, tone: tone)
                }
            }
        }
    }

    private var accessibilityLabel: String {
        let servers = profile.servers
            .map { "\($0.server) \($0.mode ?? ($0.enabled ? "on" : "off"))" }
            .joined(separator: ", ")
        return ["Profile \(profile.name)", profile.error.map { "Error: \($0)" } ?? profile.description, servers]
            .filter { !$0.isEmpty }
            .joined(separator: ". ")
    }
}
