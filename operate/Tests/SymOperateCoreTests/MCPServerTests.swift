import XCTest
import SymairaMCP
@testable import SymOperateMCP
@testable import SymOperateCore

final class MCPServerTests: XCTestCase {
    private var server: SymOperateMCP.MCPServer!

    override func setUp() {
        super.setUp()
        server = MCPServer()
    }

    func testInitializeReturnsProtocolVersion() async throws {
        let result = try await server.dispatch(method: "initialize", params: ["protocolVersion": "2024-11-05"])
        XCTAssertEqual(result["protocolVersion"] as? String, SymairaMCP.MCPServer.supportedProtocolVersion)
        let capabilities = result["capabilities"] as? [String: Any]
        XCTAssertNotNil(capabilities?["tools"])
    }

    func testPingReturnsEmptyResult() async throws {
        let result = try await server.dispatch(method: "ping", params: [:])
        XCTAssertEqual(result.count, 0)
    }

    func testToolsListReturnsAllTools() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        XCTAssertEqual(tools?.count, 20)
        let names = tools?.compactMap { $0["name"] as? String }
        XCTAssertTrue(names?.contains("snapshot") ?? false)
        XCTAssertTrue(names?.contains("click") ?? false)
        XCTAssertTrue(names?.contains("list_displays") ?? false)
        XCTAssertTrue(names?.contains("wait_for") ?? false)
    }

    func testUnknownMethodThrows() async {
        do {
            _ = try await server.dispatch(method: "invalid/method", params: [:])
            XCTFail("Expected error to be thrown")
        } catch let error as AutomationError {
            let message = error.localizedDescription
            XCTAssertTrue(message.contains("Method not found"))
        } catch {
            XCTFail("Unexpected error type: \(type(of: error))")
        }
    }

    func testQueryUISchemaHasMaxDepthAndMaxNodes() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let queryUI = tools?.first { $0["name"] as? String == "query_ui" }
        XCTAssertNotNil(queryUI)
        let schema = queryUI?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["max_depth"])
        XCTAssertNotNil(properties?["max_nodes"])
        XCTAssertNotNil(properties?["window_id"])
        XCTAssertEqual(schema?["type"] as? String, "object")
    }

    func testClickSchemaHasCorrectFields() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let click = tools?.first { $0["name"] as? String == "click" }
        let schema = click?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["snapshot_id"])
        XCTAssertNotNil(properties?["element_id"])
        XCTAssertNotNil(properties?["x"])
        XCTAssertNotNil(properties?["y"])
        XCTAssertNotNil(properties?["button"])
        XCTAssertNotNil(properties?["double_click"])
        // Verify oneOf constraint for mutually exclusive parameter groups
        let oneOf = schema?["oneOf"] as? [[String: Any]]
        XCTAssertEqual(oneOf?.count, 2)
        let coordRequired = oneOf?[0]["required"] as? [String]
        XCTAssertEqual(coordRequired?.sorted(), ["x", "y"])
        let elemRequired = oneOf?[1]["required"] as? [String]
        XCTAssertEqual(elemRequired?.sorted(), ["element_id", "snapshot_id"])
    }

    func testTypeTextToolRequiresTextArgument() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let typeText = tools?.first { $0["name"] as? String == "type_text" }
        let schema = typeText?["inputSchema"] as? [String: Any]
        let required = schema?["required"] as? [String]
        XCTAssertEqual(required, ["text"])
    }

    func testToolsCallSnapshotReturnsContent() async throws {
        let result: [String: Any]
        do {
            result = try await server.dispatch(method: "tools/call", params: ["name": "snapshot", "arguments": [:]])
        } catch let error as AutomationError {
            if case .unavailable(let message) = error,
               message.contains("not found in ScreenCaptureKit") {
                throw XCTSkip("Host has no ScreenCaptureKit-visible display; snapshot cannot run here.")
            }
            if case .permissionDenied(let message) = error,
               message.contains("Screen Recording") {
                throw XCTSkip("Host has not granted Screen Recording; snapshot cannot run here.")
            }
            throw error
        }
        let content = result["content"] as? [[String: Any]]
        XCTAssertNotNil(content)
        XCTAssertEqual(content?.first?["type"] as? String, "text")
        XCTAssertNotNil(result["structuredContent"])
        XCTAssertEqual(result["isError"] as? Bool, false)
    }

    func testToolsCallWithMissingNameThrows() async {
        do {
            _ = try await server.dispatch(method: "tools/call", params: ["arguments": [:]])
            XCTFail("Expected error to be thrown")
        } catch let error as AutomationError {
            let message = error.localizedDescription
            XCTAssertTrue(message.contains("tool name") || message.contains("requires"))
        } catch {
            XCTFail("Unexpected error type: \(type(of: error))")
        }
    }

    func testToolsCallWithInvalidToolNameThrows() async {
        do {
            _ = try await server.dispatch(method: "tools/call", params: ["name": "nonexistent_tool", "arguments": [:]])
            XCTFail("Expected error to be thrown")
        } catch let error as AutomationError {
            let message = error.localizedDescription
            XCTAssertTrue(message.contains("Unknown tool"))
        } catch {
            XCTFail("Unexpected error type: \(type(of: error))")
        }
    }

    func testListDisplaysReturnsDisplays() async throws {
        let result = try await server.dispatch(method: "tools/call", params: ["name": "list_displays", "arguments": [:]])
        let content = result["content"] as? [[String: Any]]
        XCTAssertNotNil(content)
        let text = content?.first?["text"] as? String ?? ""
        XCTAssertTrue(text.contains("displayID"), "Expected displayID in list_displays output")
    }

    func testListWindowsReturnsWindows() async throws {
        let result = try await server.dispatch(method: "tools/call", params: ["name": "list_windows", "arguments": [:]])
        let content = result["content"] as? [[String: Any]]
        XCTAssertNotNil(content)
        XCTAssertEqual(result["isError"] as? Bool, false)
    }

    func testDragSchemaHasOneOfConstraint() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let drag = tools?.first { $0["name"] as? String == "drag" }
        let schema = drag?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["snapshot_id"])
        XCTAssertNotNil(properties?["from_element_id"])
        XCTAssertNotNil(properties?["to_element_id"])
        XCTAssertNotNil(properties?["from_x"])
        XCTAssertNotNil(properties?["from_y"])
        XCTAssertNotNil(properties?["to_x"])
        XCTAssertNotNil(properties?["to_y"])
        let oneOf = schema?["oneOf"] as? [[String: Any]]
        XCTAssertEqual(oneOf?.count, 2)
        let coordRequired = oneOf?[0]["required"] as? [String]
        XCTAssertEqual(coordRequired?.sorted(), ["from_x", "from_y", "to_x", "to_y"])
        let elemRequired = oneOf?[1]["required"] as? [String]
        XCTAssertEqual(elemRequired?.sorted(), ["from_element_id", "snapshot_id", "to_element_id"])
    }

    func testLaunchAppSchemaHasAnyOfConstraint() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let launchApp = tools?.first { $0["name"] as? String == "launch_app" }
        let schema = launchApp?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["bundle_id"])
        XCTAssertNotNil(properties?["app_name"])
        let anyOf = schema?["anyOf"] as? [[String: Any]]
        XCTAssertEqual(anyOf?.count, 2)
        let bundleRequired = anyOf?[0]["required"] as? [String]
        XCTAssertEqual(bundleRequired, ["bundle_id"])
        let appRequired = anyOf?[1]["required"] as? [String]
        XCTAssertEqual(appRequired, ["app_name"])
    }

    func testFocusWindowSchemaHasAnyOfConstraint() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let focusWindow = tools?.first { $0["name"] as? String == "focus_window" }
        let schema = focusWindow?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["bundle_id"])
        XCTAssertNotNil(properties?["app_name"])
        XCTAssertNotNil(properties?["title"])
        let anyOf = schema?["anyOf"] as? [[String: Any]]
        XCTAssertEqual(anyOf?.count, 2)
        let bundleRequired = anyOf?[0]["required"] as? [String]
        XCTAssertEqual(bundleRequired, ["bundle_id"])
        let appRequired = anyOf?[1]["required"] as? [String]
        XCTAssertEqual(appRequired, ["app_name"])
    }

    func testSnapshotSchemaHasNoRequiredParams() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let snapshot = tools?.first { $0["name"] as? String == "snapshot" }
        let schema = snapshot?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]
        XCTAssertNotNil(properties?["display_id"])
        XCTAssertNotNil(properties?["window_id"])
        let required = schema?["required"] as? [String]
        XCTAssertNil(required, "snapshot should have no required parameters")
        let description = snapshot?["description"] as? String ?? ""
        XCTAssertTrue(description.contains("optional"), "snapshot description should note both parameters are optional")
    }
}
extension MCPServerTests {
    // MARK: - Permission Gate Tests (Issue #85)
    func testSetPolicyGrantedPermissionsRestrictsAndGetPolicyRoundTrips() async throws {
        let setResult = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy",
            "arguments": ["granted_permissions": ["capture", "input"]],
        ])
        XCTAssertEqual(setResult["isError"] as? Bool, false)

        let getResult = try await server.dispatch(method: "tools/call", params: ["name": "get_policy", "arguments": [:]])
        let policy = getResult["structuredContent"] as? [String: Any]
        XCTAssertEqual(policy?["granted_permissions"] as? [String], ["capture", "input"])
    }

    func testSetPolicyGrantedPermissionsReplacesNotUnions() async throws {
        _ = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy", "arguments": ["granted_permissions": ["capture", "policy_modify"]],
        ])
        _ = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy", "arguments": ["granted_permissions": ["input", "policy_modify"]],
        ])
        let getResult = try await server.dispatch(method: "tools/call", params: ["name": "get_policy", "arguments": [:]])
        let policy = getResult["structuredContent"] as? [String: Any]
        // REPLACE semantics: the earlier "capture" must be gone, not unioned in.
        XCTAssertEqual(policy?["granted_permissions"] as? [String], ["input", "policy_modify"])
    }

    func testRestrictedAgentReceivesPermissionDeniedForSnapshot() async throws {
        _ = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy", "arguments": ["granted_permissions": ["input"]],
        ])
        do {
            _ = try await server.dispatch(method: "tools/call", params: ["name": "snapshot", "arguments": [:]])
            XCTFail("Expected permissionDenied for snapshot without capture permission")
        } catch let error as AutomationError {
            if case .permissionDenied(let message) = error {
                XCTAssertTrue(message.contains("capture"), "Expected message naming 'capture', got: \(message)")
            } else {
                XCTFail("Expected permissionDenied, got \(error)")
            }
        }
    }

    func testSetPolicyRequiresPolicyModifyPermission() async throws {
        _ = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy", "arguments": ["granted_permissions": ["capture"]],
        ])
        do {
            _ = try await server.dispatch(method: "tools/call", params: [
                "name": "set_policy", "arguments": ["allow_keywords": ["save"]],
            ])
            XCTFail("Expected permissionDenied for set_policy without policy_modify permission")
        } catch let error as AutomationError {
            if case .permissionDenied(let message) = error {
                XCTAssertTrue(message.contains("policy_modify"), "Expected message naming 'policy_modify', got: \(message)")
            } else {
                XCTFail("Expected permissionDenied, got \(error)")
            }
        }
        // The refused mutation must not have changed the policy.
        let getResult = try await server.dispatch(method: "tools/call", params: ["name": "get_policy", "arguments": [:]])
        let policy = getResult["structuredContent"] as? [String: Any]
        XCTAssertEqual(policy?["granted_permissions"] as? [String], ["capture"])
        XCTAssertEqual(policy?["allowedKeywords"] as? [String], [], "Refused allow_keywords must not be applied")
    }

    func testPermissionDeniedRefusalPayloadCarriesStableCode() {
        let message = "Permission denied: the 'capture' permission is required for snapshot."
        let payload = SymOperateMCP.MCPServer.refusalPayload(
            code: AutomationError.permissionDenied(message).code,
            message: message
        )
        XCTAssertEqual(payload["isError"] as? Bool, false)
        let structured = payload["structuredContent"] as? [String: Any]
        XCTAssertEqual(structured?["status"] as? String, "refused")
        let refusal = structured?["refusal"] as? [String: Any]
        XCTAssertEqual(refusal?["code"] as? String, "destructive_control_refused")
        XCTAssertEqual(refusal?["message"] as? String, message)
    }
}

extension MCPServerTests {
    // MARK: - Wire Integration Tests (migration #97)

    /// Runs `server` over an in-memory pipe pair, feeding it `requests`
    /// (newline-delimited JSON-RPC) and returning every response line parsed
    /// as a JSON object. Exercises the real transport + dispatch path that
    /// `symoperate serve` uses, not just the in-process `dispatch` seam.
    private func runWire(server: SymOperateMCP.MCPServer, requests: [String]) async throws -> [[String: Any]] {
        let input = Pipe()
        let output = Pipe()
        let transport = MCPStdioTransport(input: input.fileHandleForReading, output: output.fileHandleForWriting)
        let task = Task {
            try await server.run(transport: transport)
        }
        for request in requests {
            input.fileHandleForWriting.write(Data((request + "\n").utf8))
        }
        input.fileHandleForWriting.closeFile()
        try await task.value
        output.fileHandleForWriting.closeFile()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        return data.split(separator: 0x0A).compactMap { line in
            try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any]
        }
    }

    func testWireInitializeUsesSupportedProtocolVersion() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        ])
        XCTAssertEqual(responses.count, 1)
        let result = responses[0]["result"] as? [String: Any]
        XCTAssertEqual(
            result?["protocolVersion"] as? String,
            SymairaMCP.MCPServer.supportedProtocolVersion
        )
        let info = result?["serverInfo"] as? [String: Any]
        XCTAssertEqual(info?["name"] as? String, "symoperate")
        XCTAssertEqual(info?["version"] as? String, SymOperateVersion.current)
    }

    func testWireToolsListReturnsAllTools() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ])
        let tools = (responses[0]["result"] as? [String: Any])?["tools"] as? [[String: Any]]
        XCTAssertEqual(tools?.count, 20)
    }

    func testWireToolsCallKeepsNumericStructuredContent() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_displays","arguments":{}}}"#,
        ])
        let result = responses[0]["result"] as? [String: Any]
        XCTAssertEqual(result?["isError"] as? Bool, false)
        let structured = result?["structuredContent"] as? [[String: Any]]
        let first = structured?.first
        XCTAssertNotNil(first, "Expected at least one display in structuredContent")
        // Regression (migration #97): the Any → MCPJSONValue bridge must not
        // turn numeric values into booleans (displayID 1 → true, bounds 0 → false).
        // Note: `is Bool` cannot be used here — NSNumber(1) itself bridges to
        // Bool in Swift; CF type identity is the reliable discrimination.
        let displayID = first?["displayID"] as? NSNumber
        XCTAssertNotNil(displayID)
        XCTAssertFalse(CFGetTypeID(displayID!) == CFBooleanGetTypeID(), "displayID must stay numeric, got \(String(describing: displayID))")
        XCTAssertEqual(displayID?.intValue, 1)
        let bounds = first?["bounds"] as? [String: Any]
        let boundsX = bounds?["x"] as? NSNumber
        XCTAssertNotNil(boundsX)
        XCTAssertFalse(CFGetTypeID(boundsX!) == CFBooleanGetTypeID(), "bounds.x must stay numeric, got \(String(describing: boundsX))")
        XCTAssertEqual(boundsX?.intValue, 0)
    }

    func testWirePermissionDeniedReturnsRefusalResult() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_policy","arguments":{"granted_permissions":["input"]}}}"#,
            #"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"snapshot","arguments":{}}}"#,
        ])
        XCTAssertEqual(responses.count, 2)
        let result = responses[1]["result"] as? [String: Any]
        XCTAssertEqual(result?["isError"] as? Bool, false)
        let structured = result?["structuredContent"] as? [String: Any]
        XCTAssertEqual(structured?["status"] as? String, "refused")
        XCTAssertNotNil((structured?["refusal"] as? [String: Any])?["code"])
    }

    func testWireNotificationGetsNoResponse() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            #"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        ])
        XCTAssertEqual(responses.count, 1, "Notifications must not produce a response")
        XCTAssertEqual((responses[0]["id"] as? NSNumber)?.intValue, 1)
    }

    func testWireGenuineFaultCarriesErrorDataCode() async throws {
        // Regression (issue #101): after the SymairaMCP migration the error
        // path dropped the machine-readable classification. A genuine
        // AutomationError must surface as a JSON-RPC -32603 error whose
        // `error.data.code` equals the AutomationError code.
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"type_text","arguments":{"text":""}}}"#,
        ])
        XCTAssertEqual(responses.count, 1)
        let error = responses[0]["error"] as? [String: Any]
        XCTAssertEqual(error?["code"] as? Int, -32603)
        let data = error?["data"] as? [String: Any]
        XCTAssertEqual(data?["code"] as? String, "invalid_argument")
    }

    func testWireUnknownMethodReturnsMethodNotFound() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"invalid/method"}"#,
        ])
        let error = responses[0]["error"] as? [String: Any]
        XCTAssertEqual(error?["code"] as? Int, -32601)
        XCTAssertTrue((error?["message"] as? String)?.contains("invalid/method") ?? false)
    }

    func testWireScrollInvalidDeltaIsClassifiedAndServerContinues() async throws {
        let responses = try await runWire(server: server, requests: [
            #"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scroll","arguments":{"delta_y":1e100}}}"#,
            #"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        ])

        XCTAssertEqual(responses.count, 2, "An invalid scroll must not terminate the MCP server")
        let error = responses[0]["error"] as? [String: Any]
        XCTAssertEqual(error?["code"] as? Int, -32603)
        let data = error?["data"] as? [String: Any]
        XCTAssertEqual(data?["code"] as? String, "invalid_argument")
        XCTAssertEqual((responses[1]["result"] as? [String: Any])?.isEmpty, true)
    }
}


extension MCPServerTests {
    func testToolCatalogNamesMatchExposedToolsInOrder() async throws {
        let definitions = SymOperateMCP.MCPServer.toolDefinitions
        let catalogNames = definitions.map(\.name)
        XCTAssertEqual(definitions.count, 20)
        XCTAssertEqual(Set(catalogNames).count, catalogNames.count, "Tool catalog names must be unique")
        XCTAssertTrue(definitions.allSatisfy { !$0.description.isEmpty })

        let result = try await server.dispatch(method: "tools/list", params: [:])
        let exposedNames = (result["tools"] as? [[String: Any]])?.compactMap { $0["name"] as? String }
        XCTAssertEqual(exposedNames, catalogNames)
    }


    // MARK: - Policy surface no longer advertises inert controls (issue #242)

    func testSetPolicySchemaDoesNotAdvertiseABundleAllowlist() async throws {
        let result = try await server.dispatch(method: "tools/list", params: [:])
        let tools = result["tools"] as? [[String: Any]]
        let setPolicy = tools?.first { $0["name"] as? String == "set_policy" }
        let schema = setPolicy?["inputSchema"] as? [String: Any]
        let properties = schema?["properties"] as? [String: Any]

        XCTAssertNotNil(properties, "set_policy should still take arguments")
        XCTAssertNil(
            properties?["allow_bundle_ids"],
            "the bundle allowlist was never consulted; it must not be advertised"
        )

        let description = (setPolicy?["description"] as? String ?? "")
            + ((properties?["granted_permissions"] as? [String: Any])?["description"] as? String ?? "")
        XCTAssertFalse(description.contains("destructive_action"), description)
        XCTAssertFalse(description.contains("secure_field_access"), description)
    }

    func testPolicyPayloadDoesNotReportABundleAllowlist() async throws {
        let result = try await server.dispatch(method: "tools/call", params: [
            "name": "get_policy", "arguments": [:],
        ])
        let policy = result["structuredContent"] as? [String: Any]
        XCTAssertNotNil(policy)
        XCTAssertNil(policy?["allowed_bundle_ids"])
        XCTAssertNil(policy?["allowedBundleIDs"])
    }

    func testSetPolicyIgnoresRetiredPermissionNamesInsteadOfRefusing() async throws {
        // A client still sending the old full list must narrow to the advertised
        // flags, not trip the "cannot widen the startup grant" guard.
        let result = try await server.dispatch(method: "tools/call", params: [
            "name": "set_policy",
            "arguments": ["granted_permissions": ["capture", "destructive_action", "secure_field_access"]],
        ])
        XCTAssertEqual(result["isError"] as? Bool, false)
        let policy = result["structuredContent"] as? [String: Any]
        XCTAssertEqual(policy?["granted_permissions"] as? [String], ["capture"])
    }
}
