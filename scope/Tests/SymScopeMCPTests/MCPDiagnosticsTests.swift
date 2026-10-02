import Foundation
import XCTest
import SymScopeCore
@testable import SymScopeMCP

private struct DiagnosticFixtureHarnessService: HarnessInventoryProviding {
    let inventory: HarnessInventory

    var isAvailable: Bool { true }
    func list(projectDir: String?) -> HarnessInventory? { inventory }
}

final class MCPDiagnosticsTests: XCTestCase {
    func testMCPListDiagnosticsUsesTypedFixtureAndOmitsRawErrors() async throws {
        let server = SymScopeMCPServer(harnessService: DiagnosticFixtureHarnessService(inventory: makeFixtureInventory()))
        let result = try await server.dispatch(
            method: "tools/call",
            params: ["name": "mcp_list", "arguments": ["diagnostics": true]]
        )
        let output = try XCTUnwrap(textResult(from: result))
        let data = Data(output.utf8)
        let report = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any]
        )
        let configurations = try XCTUnwrap(report["configurations"] as? [[String: Any]])
        XCTAssertEqual(configurations.count, 5)
        XCTAssertEqual(
            configurations.compactMap { $0["status"] as? String },
            ["ready", "empty", "unavailable", "invalid", "empty"]
        )
        XCTAssertEqual(configurations[4]["config_scope"] as? String, "project")
        XCTAssertFalse(output.contains("RAW_CONFIG_CANARY"))
        XCTAssertFalse(output.contains("RESOLVED_ENV_CANARY"))
        XCTAssertEqual((report["servers"] as? [[String: Any]])?.first?["name"] as? String, "tasker")
    }

    func testMCPListWithoutDiagnosticsRemainsAnArray() async throws {
        let server = SymScopeMCPServer(harnessService: DiagnosticFixtureHarnessService(inventory: makeFixtureInventory()))
        let result = try await server.dispatch(
            method: "tools/call",
            params: ["name": "mcp_list"]
        )
        let output = try XCTUnwrap(textResult(from: result))
        let servers = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(output.utf8)) as? [[String: Any]])
        XCTAssertEqual(servers.count, 1)
        XCTAssertEqual(servers[0]["name"] as? String, "tasker")
        XCTAssertEqual(servers[0]["config_path"] as? String, "/home/fixture/.codex/config.toml")
    }

    func testMCPListToolAdvertisesOptionalDiagnosticsArgument() async throws {
        let server = SymScopeMCPServer(harnessService: DiagnosticFixtureHarnessService(inventory: makeFixtureInventory()))
        let result = try await server.dispatch(method: "tools/list")
        let tools = try XCTUnwrap(result["tools"] as? [[String: Any]])
        let mcpList = try XCTUnwrap(tools.first { $0["name"] as? String == "mcp_list" })
        let schema = try XCTUnwrap(mcpList["inputSchema"] as? [String: Any])
        let properties = try XCTUnwrap(schema["properties"] as? [String: Any])
        XCTAssertEqual((properties["diagnostics"] as? [String: Any])?["type"] as? String, "boolean")
    }

    private func textResult(from result: [String: Any]) -> String? {
        (result["content"] as? [[String: Any]])?.first?["text"] as? String
    }

    private func makeFixtureInventory() -> HarnessInventory {
        HarnessInventory(
            schemaVersion: 2,
            projectDir: nil,
            harnesses: [
                HarnessInventoryEntry(
                    name: "codex",
                    displayName: "Codex",
                    global: HarnessConfigInventory(
                        path: "/home/fixture/.codex/config.toml",
                        exists: true,
                        parsed: true,
                        error: nil,
                        servers: [
                            HarnessServerInventory(
                                name: "tasker",
                                transport: "stdio",
                                command: "bridge",
                                args: ["mcp"],
                                envNames: ["TASKER_API_KEY"]
                            ),
                        ]
                    )
                ),
                HarnessInventoryEntry(
                    name: "claude",
                    displayName: "Claude",
                    global: HarnessConfigInventory(
                        path: "/home/fixture/.claude/settings.json",
                        exists: true,
                        parsed: true,
                        error: nil,
                        servers: []
                    )
                ),
                HarnessInventoryEntry(
                    name: "cursor",
                    displayName: "Cursor",
                    global: HarnessConfigInventory(
                        path: "/home/fixture/.cursor/mcp.json",
                        exists: false,
                        parsed: false,
                        error: nil,
                        servers: []
                    )
                ),
                HarnessInventoryEntry(
                    name: "zed",
                    displayName: "Zed",
                    global: HarnessConfigInventory(
                        path: "/home/fixture/.config/zed/settings.json",
                        exists: true,
                        parsed: false,
                        error: "invalid JSON near api_key=RAW_CONFIG_CANARY; env=RESOLVED_ENV_CANARY",
                        servers: []
                    ),
                    project: HarnessConfigInventory(
                        path: "/workspace/.zed/settings.json",
                        exists: true,
                        parsed: true,
                        error: nil,
                        servers: []
                    )
                ),
            ]
        )
    }
}
