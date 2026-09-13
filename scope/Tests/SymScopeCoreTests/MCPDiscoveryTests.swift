import XCTest
@testable import SymScopeCore

private struct FakeHarnessService: HarnessInventoryProviding {
    let available: Bool
    let inventory: HarnessInventory?

    var isAvailable: Bool { available }
    func list(projectDir: String?) -> HarnessInventory? { inventory }
}

final class MCPDiscoveryTests: XCTestCase {
    func testDiscoverRequiresSymbrainWhenUnavailable() {
        let fake = FakeHarnessService(available: false, inventory: nil)
        let (servers, notes) = MCPDiscovery.discover(harnessService: fake)

        XCTAssertTrue(servers.isEmpty)
        XCTAssertEqual(notes, [MCPDiscovery.requiresSymbrainNote])
    }

    func testDiscoverRequiresSymbrainWhenListFails() {
        let fake = FakeHarnessService(available: true, inventory: nil)
        let (servers, notes) = MCPDiscovery.discover(harnessService: fake)

        XCTAssertTrue(servers.isEmpty)
        XCTAssertTrue(notes[0].contains("requires symbrain"))
    }

    func testDiscoverRejectsUnsupportedSchema() {
        let inventory = HarnessInventory(schemaVersion: 1, projectDir: nil, harnesses: [])
        let fake = FakeHarnessService(available: true, inventory: inventory)
        let (servers, notes) = MCPDiscovery.discover(harnessService: fake)

        XCTAssertTrue(servers.isEmpty)
        XCTAssertTrue(notes[0].contains("schema 2"))
    }

    func testDiscoverUsesSymbrainSchema2ServerDetails() {
        let inventory = HarnessInventory(
            schemaVersion: 2,
            projectDir: nil,
            harnesses: [
                HarnessInventoryEntry(
                    name: "codex",
                    displayName: "Codex CLI",
                    global: HarnessConfigInventory(
                        path: "/home/user/.codex/config.toml",
                        exists: true,
                        parsed: true,
                        error: nil,
                        servers: [
                            HarnessServerInventory(
                                name: "linear",
                                transport: "http",
                                url: "https://mcp.linear.app/mcp"
                            ),
                            HarnessServerInventory(
                                name: "local",
                                transport: "stdio",
                                command: "symbrain",
                                args: ["mcp", "--profile", "personal"]
                            ),
                        ]
                    )
                )
            ]
        )
        let fake = FakeHarnessService(available: true, inventory: inventory)
        let (servers, notes) = MCPDiscovery.discover(harnessService: fake)

        XCTAssertEqual(servers.count, 2)
        XCTAssertEqual(servers.map(\.name), ["linear", "local"])
        XCTAssertEqual(servers[0].transport, "http")
        XCTAssertEqual(servers[0].url, "https://mcp.linear.app/mcp")
        XCTAssertEqual(servers[1].command, "symbrain")
        XCTAssertEqual(servers[1].args, ["mcp", "--profile", "personal"])
        XCTAssertTrue(servers.allSatisfy { $0.source == "symbrain" })
        XCTAssertTrue(notes.contains { $0.hasPrefix("mcp: source=symbrain") })
    }

    func testDiscoverSurfacesSymbrainConfigErrorsAsNotes() {
        let inventory = HarnessInventory(
            schemaVersion: 2,
            projectDir: nil,
            harnesses: [
                HarnessInventoryEntry(
                    name: "cursor",
                    displayName: "Cursor",
                    global: HarnessConfigInventory(
                        path: "/home/user/config.json",
                        exists: true,
                        parsed: false,
                        error: "invalid JSON",
                        servers: []
                    )
                )
            ]
        )
        let fake = FakeHarnessService(available: true, inventory: inventory)
        let (servers, notes) = MCPDiscovery.discover(harnessService: fake)

        XCTAssertTrue(servers.isEmpty)
        XCTAssertTrue(notes.contains { $0.contains("invalid JSON") })
    }
}
