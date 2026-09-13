import Foundation

/// Aggregate snapshot: ports + MCP servers + containers + background services.
/// Mirrors the Go original's scan.Build (concurrent collection, notes for
/// non-fatal degradations).
public struct SnapshotService: Sendable {
    public static func build() async -> Snapshot {
        await build(portReport: { await PortService.listListeningReport() })
    }

    /// The port inventory is injectable so a test can prove it is collected
    /// exactly once. Two consumers want the same list — this snapshot and
    /// `DaemonService`, which annotates each daemon with the ports its PID
    /// holds — and `DaemonService` used to collect its own, running a second
    /// pair of `lsof` children whose result was then discarded here in favour
    /// of this one. Now there is a single inventory and both await it.
    static func build(
        portReport: @escaping @Sendable () async -> (ports: [Port], notes: [String])
    ) async -> Snapshot {
        let portsTask = Task { await portReport() }

        async let containersTask: (containers: [Container], notes: [String]) = {
            let (containers, notes) = await ContainerService.list()
            return (containers, notes)
        }()
        // Daemons is the long pole (`brew services list`), so it starts before
        // the synchronous symbrain discovery below rather than after it. It
        // only needs the ports at the very end, by which time the shared
        // inventory has long since finished.
        async let daemonsTask = DaemonService(
            portProvider: { await portsTask.value.ports }
        ).list()

        let (servers, serverNotes) = MCPDiscovery.discover()

        let portsResult = await portsTask.value
        let daemonsResult = await daemonsTask
        let containersResult = await containersTask

        // No second `annotatePorts` pass: `DaemonService.list` already annotated
        // these rows, from this very inventory.
        let daemons = daemonsResult.0
        let notes = portsResult.notes + serverNotes + containersResult.notes + daemonsResult.1

        // ISO8601 generated_at, matching the Go original's layout.
        let formatter = ISO8601DateFormatter()
        let generatedAt = formatter.string(from: Date())

        return Snapshot(
            generatedAt: generatedAt,
            ports: portsResult.ports,
            mcpServers: servers,
            containers: containersResult.containers,
            daemons: daemons,
            notes: notes
        )
    }
}
