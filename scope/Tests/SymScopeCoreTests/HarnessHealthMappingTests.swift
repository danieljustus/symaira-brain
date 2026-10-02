import Foundation
import XCTest
@testable import SymScopeCore

final class HarnessHealthMappingTests: XCTestCase {
    private func report(_ json: String) throws -> HarnessHealthReport {
        try JSONDecoder().decode(HarnessHealthReport.self, from: Data(json.utf8))
    }

    func testMeasuredHealthPreservesFractionalLatencyMethodAndOutcomes() throws {
        let source = try report("""
        {"health_schema_version":1,"servers":[
          {"harness":"codex","config":"fixture","server":"http","transport":"http",
           "healthy":true,"outcome":"healthy","probe_method":"initialize+ping","latency_ms":0.625},
          {"harness":"codex","config":"fixture","server":"failed","transport":"stdio",
           "healthy":false,"outcome":"unhealthy","probe_method":"initialize","latency_ms":5.25,"error":"MCP initialize failed"},
          {"harness":"codex","config":"fixture","server":"auth","transport":"http",
           "healthy":false,"outcome":"unsupported","error":"server has configured environment values"},
          {"harness":"codex","config":"fixture","server":"missing","transport":"stdio",
           "healthy":false,"outcome":"unhealthy","error":"server command was not found"}
        ]}
        """)
        let mapped = try XCTUnwrap(SymBrainHarnessService.mapHealth(source))
        XCTAssertEqual(mapped.map(\.status), ["healthy", "unhealthy", "unsupported", "unhealthy"])
        XCTAssertEqual(mapped[0].latencyMs, 0.625)
        XCTAssertEqual(mapped[0].probeMethod, "initialize+ping")
        XCTAssertEqual(mapped[0].healthSchemaVersion, 1)
        XCTAssertEqual(mapped[1].latencyMs, 5.25)
        XCTAssertEqual(mapped[1].probeMethod, "initialize")
        XCTAssertEqual(mapped[1].error, "MCP initialize failed")
        XCTAssertNil(mapped[2].latencyMs)
        XCTAssertNil(mapped[2].probeMethod)
        XCTAssertNil(mapped[3].latencyMs)
    }

    func testLegacyBrainHealthDoesNotInventAMeasurement() throws {
        let source = try report("""
        {"servers":[{"harness":"codex","config":"fixture","server":"legacy",
        "transport":"stdio","healthy":true}]}
        """)
        let mapped = try XCTUnwrap(SymBrainHarnessService.mapHealth(source))
        XCTAssertEqual(mapped[0].status, "healthy")
        XCTAssertNil(mapped[0].latencyMs)
        XCTAssertNil(mapped[0].probeMethod)
        XCTAssertNil(mapped[0].healthSchemaVersion)
    }

    func testEmptyAndNullServerReportsMapToEmptyArrays() throws {
        for source in ["{\"servers\":null}", "{\"servers\":[]}"] {
            XCTAssertEqual(try SymBrainHarnessService.mapHealth(report(source)), [])
        }
    }

    func testUnknownHealthSchemaIsRejectedRatherThanGuessed() throws {
        XCTAssertNil(try SymBrainHarnessService.mapHealth(report("{\"health_schema_version\":2,\"servers\":[]}")))
    }

    func testUnprobedOutputOmitsLatencyAndMethodInsteadOfEncodingZero() throws {
        let value = MCPHealthResult(name: "auth", client: "codex", status: "unsupported", latencyMs: nil, healthSchemaVersion: 1)
        let output = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(value)) as? [String: Any])
        XCTAssertEqual(output["health_schema_version"] as? Int, 1)
        XCTAssertEqual(output["status"] as? String, "unsupported")
        XCTAssertNil(output["latency_ms"])
        XCTAssertNil(output["probe_method"])
    }

    func testLegacyScopeIntegerLatencyStillDecodes() throws {
        let source = "{\"name\":\"legacy\",\"client\":\"codex\",\"status\":\"healthy\",\"latency_ms\":12}"
        let value = try JSONDecoder().decode(MCPHealthResult.self, from: Data(source.utf8))
        XCTAssertEqual(value.latencyMs, 12)
        XCTAssertNil(value.probeMethod)
        XCTAssertNil(value.healthSchemaVersion)
    }
}
