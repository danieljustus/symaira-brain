import SwiftUI
import SymairaTheme
import SymBrainCore

/// Activity: every tool call the gateway routed, across all state cores.
///
/// Screen card — Destination: Activity (7 of 7). Job: find what an agent
/// called, through which profile, and whether it worked; no primary action.
/// Data: local JSONL audit log (`~/.local/share/symbrain/audit`), newest
/// 500 matching entries, read on open, on filter change and ⌘R. States:
/// reading the log, no activity, no match for the filter. Density: sortable
/// table. Platform: macOS workspace. Replaces the per-core activity tabs.
struct ActivityPage: View {
    @StateObject private var vm: AuditViewModel
    @State private var hasLoaded = false
    @State private var serverFilter = ""
    @State private var profileFilter = ""
    @State private var sortOrder = [KeyPathComparator(\AuditEntry.timestamp, order: .reverse)]
    @State private var selection: AuditEntry.ID?

    /// Server aliases the gateway records (`CORE_SERVER_ALIASES` in symbrain-policy).
    private static let servers = ["vault", "memory", "skills", "usage", "operate", "scope"]

    init(client: SymBrainClient) {
        _vm = StateObject(wrappedValue: AuditViewModel(client: client))
    }

    var body: some View {
        SymairaPage(
            title: "Activity",
            subtitle: "Tool calls routed through the gateway, newest first",
            scrolls: false,
            maxWidth: PageWidth.table
        ) {
            SymairaFilterMenu(
                "Server", selection: $serverFilter,
                options: [(value: "", label: "All Servers")] + Self.servers.map { (value: $0, label: $0.capitalized) }
            )
            .frame(width: 170)
            SymairaFilterMenu(
                "Profile", selection: $profileFilter,
                options: [(value: "", label: "All Profiles")] + vm.profiles.map { (value: $0.name, label: $0.name) }
            )
            .frame(width: 190)
            Spacer()
            Button(action: refresh) { Label("Refresh", symairaIcon: .refresh) }
                .symairaButtonStyle(.secondary)
                .disabled(vm.isLoading)
        } content: {
            SymairaPageState(
                state: PageLoad.state(hasLoaded: hasLoaded, error: nil, value: vm.entries),
                loading: "Reading the audit log…", retry: refresh
            ) { entries in
                if entries.isEmpty {
                    if serverFilter.isEmpty && profileFilter.isEmpty {
                        SymairaPageEmpty(
                            icon: .history,
                            title: "No Activity Yet",
                            message: "Calls appear here once a harness routes a tool call through Symaira Brain."
                        )
                    } else {
                        SymairaPageEmpty(
                            icon: .filter,
                            title: "No Matching Calls",
                            message: "No logged call matches this server and profile. Choose All Servers or All Profiles to widen the list."
                        )
                    }
                } else {
                    table(entries.sorted(using: sortOrder))
                }
            }
        }
        .task {
            await vm.loadProfiles()
            await vm.refresh()
            hasLoaded = true
        }
        .pageRefresh(refresh)
        .pagePrimaryAction(nil)
        .onChange(of: serverFilter) { _, value in
            vm.selectedServer = value.isEmpty ? nil : value
            refresh()
        }
        .onChange(of: profileFilter) { _, value in
            vm.selectedProfile = value.isEmpty ? nil : value
            refresh()
        }
    }

    private func refresh() {
        Task { await vm.refresh() }
    }

    private func table(_ entries: [AuditEntry]) -> some View {
        Table(entries, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("Time", value: \.timestamp) { entry in
                Text(entry.formattedTime).symairaText(.monoSmall)
            }
            .width(min: 140, ideal: 170)
            TableColumn("Profile", value: \.profile) { entry in
                Text(verbatim: entry.profile).symairaText(.body)
            }
            .width(min: 80, ideal: 110)
            TableColumn("Server", value: \.server) { entry in
                Text(verbatim: entry.server).symairaText(.body)
            }
            .width(min: 70, ideal: 90)
            TableColumn("Tool", value: \.tool) { entry in
                Text(verbatim: entry.tool).symairaText(.mono)
            }
            .width(min: 160, ideal: 240)
            TableColumn("Status", value: \.status) { entry in
                SymairaStatusLabel(entry.status, tone: StatusTone.call(entry.status))
            }
            .width(min: 80, ideal: 100)
            TableColumn("Duration", value: \.durationMs) { entry in
                Text("\(entry.durationMs) ms")
                    .symairaText(.monoSmall)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .width(min: 70, ideal: 90)
        }
        .contextMenu(forSelectionType: AuditEntry.ID.self) { ids in
            if let entry = entries.first(where: { ids.contains($0.id) }) {
                Button("Copy Tool Name") { Pasteboard.copy(entry.tool) }
                Button("Show Only \(entry.server.capitalized)") { serverFilter = entry.server }
                Button("Show Only Profile \(entry.profile)") { profileFilter = entry.profile }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: SymairaRadius.card, style: .continuous))
    }
}
