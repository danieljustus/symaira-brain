import Foundation
import XCTest
@testable import SymScopeCore

private struct DiagnosticFixtureHarnessService: HarnessInventoryProviding {
    let inventory: HarnessInventory

    var isAvailable: Bool { true }
    func list(projectDir: String?) -> HarnessInventory? { inventory }
}

final class MCPDiscoveryDiagnosticTests: XCTestCase {
    func testFixtureProducesTypedStatesWithoutEchoingSensitiveErrors() throws {
        let inventory = try loadFixture()
        let service = DiagnosticFixtureHarnessService(inventory: inventory)
        let (diagnostics, notes) = MCPDiscovery.diagnose(harnessService: service)

        XCTAssertFalse(notes.contains { $0.contains("RAW_CONFIG_CANARY") })
        XCTAssertNotNil(diagnostics)
        let report = try XCTUnwrap(diagnostics)
        XCTAssertEqual(report.servers.map(\.name), ["tasker"])
        XCTAssertEqual(report.servers[0].credentialWarnings, ["requires environment variable TASKER_API_KEY"])
        XCTAssertEqual(report.configurations.count, 5)

        let states = Dictionary(
            uniqueKeysWithValues: report.configurations.map {
                ("\($0.client).\($0.configScope.rawValue)", $0.status)
            }
        )
        XCTAssertEqual(states["codex.global"], .ready)
        XCTAssertEqual(states["claude.global"], .empty)
        XCTAssertEqual(states["cursor.global"], .unavailable)
        XCTAssertEqual(states["zed.global"], .invalid)
        XCTAssertEqual(states["zed.project"], .empty)
        XCTAssertEqual(report.configurations.first { $0.client == "zed" && $0.configScope == .global }?.message,
                       "Brain could not parse this configuration.")

        let output = String(decoding: try JSONEncoder().encode(report), as: UTF8.self)
        XCTAssertFalse(output.contains("RAW_CONFIG_CANARY"))
        XCTAssertFalse(output.contains("RESOLVED_ENV_CANARY"))
        XCTAssertTrue(output.contains("TASKER_API_KEY"))
        XCTAssertTrue(report.configurations.first { $0.client == "zed" && $0.configScope == .project }?.path.hasSuffix("/.zed/settings.json") == true)
    }

    func testDiagnosticPathIsControlSafeAndBoundedByUTF8Bytes() {
        let diagnostic = MCPConfigDiagnostic(
            client: "fixture",
            configScope: .global,
            status: .invalid,
            path: "/fixture/" + String(repeating: "é", count: 400) + "\nunsafe"
        )

        XCTAssertLessThanOrEqual(diagnostic.path.utf8.count, MCPDiagnosticPath.maximumUTF8Length)
        XCTAssertFalse(diagnostic.path.contains("\n"))
        XCTAssertTrue(diagnostic.path.hasSuffix("…"))
    }

    private func loadFixture() throws -> HarnessInventory {
        try JSONDecoder().decode(
            HarnessInventory.self,
            from: Data(MCPDiscoveryDiagnosticFixture.inventoryJSON.utf8)
        )
    }
}
