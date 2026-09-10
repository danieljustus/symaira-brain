import XCTest
@testable import SymScopeCore

final class SnapshotServiceTests: XCTestCase {
    func testBuildAggregatesRealSubsystemState() async throws {
        let snapshot = await SnapshotService.build()

        // generatedAt is a valid ISO8601 timestamp close to "now" — proves the
        // formatter branch actually ran rather than returning a placeholder.
        let formatter = ISO8601DateFormatter()
        let generated = try XCTUnwrap(formatter.date(from: snapshot.generatedAt))
        XCTAssertLessThan(abs(generated.timeIntervalSinceNow), 10)

        // mcpServers mirrors what MCPDiscovery independently reports — proves the
        // snapshot is a real aggregation of the discovery service, not a stub.
        let (expectedServers, expectedServerNotes) = MCPDiscovery.discover()
        XCTAssertEqual(snapshot.mcpServers, expectedServers)
        for note in expectedServerNotes {
            XCTAssertTrue(snapshot.notes.contains(note))
        }

        // containers mirrors ContainerService's own report.
        let (expectedContainers, expectedContainerNotes) = await ContainerService.list()
        XCTAssertEqual(snapshot.containers.count, expectedContainers.count)
        for note in expectedContainerNotes {
            XCTAssertTrue(snapshot.notes.contains(note))
        }

        // ports come from a concurrent task that never throws in practice on a
        // healthy machine; either it succeeded (no "ports:" note) or it
        // degraded gracefully with a note instead of crashing the whole build.
        let hasPortsFailureNote = snapshot.notes.contains { $0.hasPrefix("ports:") }
        if !hasPortsFailureNote {
            XCTAssertTrue(snapshot.ports.allSatisfy { $0.port > 0 })
        }
    }

    func testBuildCollectsThePortInventoryExactlyOnce() async {
        // Two consumers want the same list: the snapshot itself, and
        // DaemonService, which annotates every daemon with the ports its PID
        // holds. Collecting it twice meant a second pair of lsof children whose
        // result was thrown away. This is the guard against that coming back —
        // a count, not a duration, so it says the same thing on every machine.
        let collector = PortCollectorSpy()

        let snapshot = await SnapshotService.build(portReport: {
            collector.record()
            return ([Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 4242, process: "fixture")], [])
        })

        XCTAssertEqual(collector.count, 1, "the port inventory must be collected once and shared")
        XCTAssertEqual(snapshot.ports.map(\.port), [8080], "the snapshot reports the inventory it collected")
    }

    func testBuildProducesFreshTimestampsAcrossCalls() async throws {
        let first = await SnapshotService.build()
        try await Task.sleep(nanoseconds: 1_100_000_000)
        let second = await SnapshotService.build()

        let formatter = ISO8601DateFormatter()
        let firstDate = try XCTUnwrap(formatter.date(from: first.generatedAt))
        let secondDate = try XCTUnwrap(formatter.date(from: second.generatedAt))
        XCTAssertGreaterThan(secondDate, firstDate)
    }
}

/// Counts how many times the injected port collector was asked to run.
private final class PortCollectorSpy: @unchecked Sendable {
    private let lock = NSLock()
    private var calls = 0

    func record() {
        lock.lock()
        calls += 1
        lock.unlock()
    }

    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return calls
    }
}
