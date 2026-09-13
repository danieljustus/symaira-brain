import XCTest
@testable import SymScopeCore

final class WatchServiceTests: XCTestCase {
    func testDetectsPortBoundAndUnbound() {
        let portA = Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node")
        let portB = Port(port: 9090, protocol_: "tcp", address: "127.0.0.1", pid: 200, process: "python")

        let old = Snapshot(ports: [portA])
        let new = Snapshot(ports: [portB])

        let events = WatchService.diff(old: old, new: new)
        let types = Set(events.map(\.type))
        XCTAssertTrue(types.contains("port_bound"))
        XCTAssertTrue(types.contains("port_unbound"))
    }

    func testDetectsMCPAddedAndRemoved() {
        let server = MCPServer(name: "github", client: "cursor", transport: "stdio", command: "gh", configPath: "/tmp/mcp.json")
        let old = Snapshot(mcpServers: [])
        let new = Snapshot(mcpServers: [server])

        let events = WatchService.diff(old: old, new: new)
        XCTAssertTrue(events.contains { $0.type == "mcp_server_added" })
    }

    func testDetectsContainerStartedAndStopped() {
        let container = Container(id: "abc", name: "postgres", image: "postgres:17", ports: [5432])
        let old = Snapshot(containers: [container])
        let new = Snapshot(containers: [])

        let events = WatchService.diff(old: old, new: new)
        XCTAssertTrue(events.contains { $0.type == "container_stopped" })
    }

    func testNoEventsWhenIdentical() {
        let snapshot = Snapshot(
            ports: [Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 1, process: "x")],
            mcpServers: [MCPServer(name: "s", client: "c", transport: "stdio", command: "cmd", configPath: "/p")],
            containers: [Container(id: "1", name: "n", image: "i")]
        )
        XCTAssertEqual(WatchService.diff(old: snapshot, new: snapshot), [])
    }

    func testDetectsConflictChanged() {
        let oldPorts = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node"),
        ]
        let newPorts = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node"),
            Port(port: 8080, protocol_: "tcp", address: "0.0.0.0", pid: 200, process: "python"),
        ]
        let events = WatchService.diff(old: Snapshot(ports: oldPorts), new: Snapshot(ports: newPorts))
        XCTAssertTrue(events.contains { $0.type == "conflict_detected" })
    }
}

final class ExplainServiceTests: XCTestCase {
    func testExplainPortFindsHolders() {
        let ports = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node"),
        ]
        let explanation = ExplainService.explainPort(8080, ports: ports, servers: [])
        XCTAssertEqual(explanation.holders.count, 1)
        XCTAssertEqual(explanation.holders[0].process, "node")
    }

    func testExplainPortEmpty() {
        let explanation = ExplainService.explainPort(9999, ports: [], servers: [])
        XCTAssertTrue(explanation.holders.isEmpty)
    }

    func testExplainServerFound() {
        let servers = [
            MCPServer(name: "github", client: "cursor", transport: "stdio", command: "gh", configPath: "/tmp/x.json"),
        ]
        let explanation = ExplainService.explainServer("github", servers: servers)
        XCTAssertNotNil(explanation)
        XCTAssertEqual(explanation?.client, "cursor")
    }

    func testExplainServerMissing() {
        XCTAssertNil(ExplainService.explainServer("nope", servers: []))
    }
}
