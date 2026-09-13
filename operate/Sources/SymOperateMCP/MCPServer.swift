import Foundation
import SymOperateCore
import SymairaMCP

/// MCP server for `symoperate`, built on appkit's `SymairaMCP` module.
///
/// The JSON-RPC 2.0 loop (parse → validate → dispatch → respond), the
/// method-handler registry, and the stdio transport are provided by
/// `SymairaMCP.MCPServer` + `MCPStdioTransport` — the shared implementation
/// that replaced this package's hand-rolled server (see `symaira-appkit`
/// issue #55, migration issue #97). This type owns only the symoperate-
/// specific parts:
///
/// - the tool registry (`tools()`) and the `tools/call` implementations,
///   which drive `AutomationController`;
/// - the safety-refusal payload shape (`refusalPayload`): classified
///   `.permissionDenied` outcomes are returned as successful `tools/call`
///   results with a stable `structuredContent.refusal.code`, never as
///   JSON-RPC errors;
/// - the in-process `dispatch(method:params:)` seam used by unit tests and
///   embedders, which shares the exact same tool implementations as the
///   wire handlers.
///
/// The wire payload shapes are intentionally byte-compatible with the
/// previous implementation (same `tools/list` schemas including `oneOf` /
/// `anyOf` / `enum` constraints, same `structuredContent` extension key on
/// `tools/call` results, and genuine `AutomationError` faults returned as
/// JSON-RPC `-32603` errors carrying the machine-readable classification in
/// `error.data.code`). `SymairaMCP`'s generic `MCPJSONValue` result type
/// makes this possible without constraining the tool metadata to the
/// module's minimal `MCPTool` schema shape.
public final class MCPServer: @unchecked Sendable {
    private let controller: AutomationController
    private let server: SymairaMCP.MCPServer

    public init(controller: AutomationController = AutomationController()) {
        self.controller = controller
        self.server = SymairaMCP.MCPServer(name: "symoperate", version: SymOperateVersion.current)
        registerHandlers()
    }

    /// Serves MCP over stdio until stdin closes.
    public func run(transport: any MCPTransport = MCPStdioTransport()) async throws {
        try await server.start(transport: transport)
    }

    /// In-process dispatch, sharing the same tool implementations as the
    /// wire handlers. Returns the JSON-RPC result object, or throws
    /// `AutomationError` — the error-to-wire mapping (JSON-RPC errors vs.
    /// refusal results) happens in the wire handlers.
    public func dispatch(method: String, params: [String: Any]) async throws -> [String: Any] {
        switch method {
        case "initialize":
            return initializeResult()
        case "notifications/initialized":
            return [:]
        case "ping":
            return [:]
        case "tools/list":
            return ["tools": tools()]
        case "tools/call":
            guard let name = params["name"] as? String else {
                throw AutomationError.invalidArgument("tools/call requires a tool name.")
            }
            let arguments = params["arguments"] as? [String: Any] ?? [:]
            return try await callTool(name: name, arguments: arguments)
        default:
            throw AutomationError.invalidArgument("Method not found: \(method)")
        }
    }

    /// Registers the symoperate methods on appkit's server. `initialize` is
    /// overridden so the server always advertises the protocol version it
    /// supports, regardless of the client's requested version.
    private func registerHandlers() {
        server
            .withMethodHandler("initialize") { [self] (_: MCPInitializeParams) async throws -> MCPJSONValue in
                Self.jsonValue(initializeResult())
            }
            .withMethodHandler("tools/list") { [self] (_: MCPNoParams) async throws -> MCPJSONValue in
                .object(["tools": .array(tools().map(Self.jsonValue))])
            }
            .withMethodHandler("tools/call") { [self] (params: MCPCallToolParams) async throws -> MCPJSONValue in
                let arguments = (params.arguments ?? [:]).mapValues(Self.jsonAny)
                do {
                    return Self.jsonValue(try await callTool(name: params.name, arguments: arguments))
                } catch let error as AutomationError {
                    if case .permissionDenied = error {
                        // Deliberate safety refusals are NOT JSON-RPC errors —
                        // the call executed correctly and the outcome is the
                        // refusal.
                        return Self.jsonValue(Self.refusalPayload(code: error.code, message: error.localizedDescription))
                    }
                    // Genuine faults surface as JSON-RPC internal errors,
                    // carrying the stable machine-readable classification in
                    // `error.data.code` (same contract as before the
                    // SymairaMCP migration).
                    throw MCPError(error.localizedDescription, data: .object(["code": .string(error.code)]))
                } catch {
                    throw MCPError(error.localizedDescription)
                }
            }
    }

    private func initializeResult() -> [String: Any] {
        [
            "protocolVersion": SymairaMCP.MCPServer.supportedProtocolVersion,
            "capabilities": [
                "tools": [
                    "listChanged": false,
                ],
            ],
            "serverInfo": [
                "name": "symoperate",
                "version": SymOperateVersion.current,
            ],
        ]
    }

    /// The complete Operate MCP tool catalog. Each entry owns both the
    /// schema advertised by `tools/list` and the handler used by `tools/call`.
    /// Keep this table as the only per-tool name/schema/dispatch inventory.
    static let toolDefinitions: [ToolDefinition] = [
        ToolDefinition(
            name: "list_apps",
            description: "List currently running GUI apps on macOS.",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { server, _ in server.controller.listApps() }
        ),
        ToolDefinition(
            name: "list_windows",
            description: "List currently visible windows.",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { server, _ in server.controller.listWindows() }
        ),
        ToolDefinition(
            name: "list_displays",
            description: "List all connected displays with bounds and IDs.",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { server, _ in server.controller.listDisplays() }
        ),
        ToolDefinition(
            name: "snapshot",
            description: "Capture a display or window as PNG plus coordinate transform metadata. Both parameters are optional: omit both for the main display, provide display_id for a specific display, or provide window_id for a specific window. When window_id is provided, display_id is ignored.",
            inputSchema: { _ in [
                "type": "object",
                "properties": [
                    "display_id": ["type": "integer", "description": "Display ID to capture. Omit for main display."],
                    "window_id": ["type": "integer", "description": "Window ID to capture. When provided, display_id is ignored."],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_title": ["type": "string"],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                try server.controller.snapshot(
                    displayID: server.uint32(arguments["display_id"]),
                    windowID: server.intOptional(arguments["window_id"]),
                    target: server.targetIdentity(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "query_ui",
            description: "Capture a screenshot and accessible UI tree. With no window_id, query the frontmost app; with window_id, query that window in its owning process without falling back to the frontmost app.",
            inputSchema: { _ in [
                "type": "object",
                "properties": [
                    "max_depth": ["type": "integer", "default": 4],
                    "max_nodes": ["type": "integer", "default": 200],
                    "display_id": ["type": "integer", "description": "Display ID to capture. Omit for main display."],
                    "window_id": ["type": "integer", "description": "Window ID to capture. When provided, display_id is ignored."],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_title": ["type": "string"],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                try server.controller.queryUI(
                    maxDepth: server.int(arguments["max_depth"], default: 4),
                    maxNodes: server.int(arguments["max_nodes"], default: 200),
                    displayID: server.uint32(arguments["display_id"]),
                    windowID: server.intOptional(arguments["window_id"]),
                    target: server.targetIdentity(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "query_ui_ocr",
            description: "Like query_ui but falls back to Vision OCR when the Accessibility tree is weak. Returns OCR text regions with coordinates.",
            inputSchema: { _ in [
                "type": "object",
                "properties": [
                    "max_depth": ["type": "integer", "default": 4],
                    "max_nodes": ["type": "integer", "default": 200],
                    "display_id": ["type": "integer", "description": "Display ID to capture. Omit for main display."],
                    "window_id": ["type": "integer", "description": "Window ID to capture. When provided, display_id is ignored."],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_title": ["type": "string"],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                try server.controller.queryUIWithOCR(
                    maxDepth: server.int(arguments["max_depth"], default: 4),
                    maxNodes: server.int(arguments["max_nodes"], default: 200),
                    displayID: server.uint32(arguments["display_id"]),
                    windowID: server.intOptional(arguments["window_id"]),
                    target: server.targetIdentity(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "find_ui",
            description: "Search the UI tree by role, title, label, value, subrole, or actions. Supports regex patterns (wrap in /slashes/). When snapshot_id is provided and the snapshot is still cached, reuses the existing snapshot instead of taking a fresh one.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "role": ["type": "string"],
                    "title": ["type": "string"],
                    "label": ["type": "string"],
                    "value": ["type": "string"],
                    "subrole": ["type": "string"],
                    "actions": ["type": "array", "items": ["type": "string"]],
                    "max_depth": ["type": "integer", "default": 4],
                    "max_nodes": ["type": "integer", "default": 200],
                    "display_id": ["type": "integer", "description": "Display ID to capture. Omit for main display."],
                    "window_id": ["type": "integer", "description": "Window ID to query in its owning process. Omit to query the frontmost app."],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_title": ["type": "string"],
                    "snapshot_id": ["type": "string", "description": "Reuse an existing snapshot only when its target scope matches the request."],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                let predicate = UIElementPredicate(
                    role: server.string(arguments["role"]),
                    title: server.string(arguments["title"]),
                    label: server.string(arguments["label"]),
                    value: server.string(arguments["value"]),
                    subrole: server.string(arguments["subrole"]),
                    actions: arguments["actions"] as? [String]
                )
                return try server.controller.findUI(
                    predicate: predicate,
                    snapshotID: server.string(arguments["snapshot_id"]),
                    maxDepth: server.int(arguments["max_depth"], default: 4),
                    maxNodes: server.int(arguments["max_nodes"], default: 200),
                    displayID: server.uint32(arguments["display_id"]),
                    windowID: server.intOptional(arguments["window_id"]),
                    target: server.targetIdentity(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "click",
            description: "Click by x/y coordinates or by snapshot_id + element_id. Requires exactly one of two groups: (x, y) for coordinate-based clicking, or (snapshot_id, element_id) for element-based clicking. Raw coordinates require a prior query_ui snapshot so the target element can be identified and safety-checked; destructive controls and secure text fields are always blocked. Optional: button (default \"left\"), double_click.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "snapshot_id": ["type": "string"],
                    "element_id": ["type": "string"],
                    "x": ["type": "number"],
                    "y": ["type": "number"],
                    "button": ["type": "string", "enum": ["left", "right"]],
                    "double_click": ["type": "boolean"],
                    "delivery_mode": ["type": "string", "enum": ["automatic", "background", "foreground"]],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "oneOf": [
                    ["required": ["x", "y"]],
                    ["required": ["snapshot_id", "element_id"]],
                ],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.click(
                    snapshotID: server.string(arguments["snapshot_id"]),
                    elementID: server.string(arguments["element_id"]),
                    x: server.double(arguments["x"]),
                    y: server.double(arguments["y"]),
                    button: server.string(arguments["button"]) ?? "left",
                    doubleClick: server.bool(arguments["double_click"], default: false),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "type_text",
            description: "Type raw unicode text into the current focused control.",
            inputSchema: { server in [
                "type": "object",
                "properties": ["text": ["type": "string"], "precondition": server.predicateSchema(), "postcondition": server.predicateSchema(), "delivery_mode": ["type": "string", "enum": ["automatic", "background", "foreground"]], "process_id": ["type": "integer"], "bundle_id": ["type": "string"], "app_name": ["type": "string"], "window_id": ["type": "integer"], "window_title": ["type": "string"]],
                "required": ["text"],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.typeText(
                    server.requireString(arguments["text"], name: "text"),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "press_keys",
            description: "Press a keyboard shortcut like [\"cmd\", \"s\"] or [\"return\"].",
            inputSchema: { server in [
                "type": "object",
                "properties": ["keys": ["type": "array", "items": ["type": "string"]], "precondition": server.predicateSchema(), "postcondition": server.predicateSchema(), "delivery_mode": ["type": "string", "enum": ["automatic", "background", "foreground"]], "process_id": ["type": "integer"], "bundle_id": ["type": "string"], "app_name": ["type": "string"], "window_id": ["type": "integer"], "window_title": ["type": "string"]],
                "required": ["keys"],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.pressKeys(
                    server.requireStringArray(arguments["keys"], name: "keys"),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "scroll",
            description: "Scroll by pixel deltas.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "delta_x": ["type": "number", "default": 0],
                    "delta_y": ["type": "number"],
                    "delivery_mode": ["type": "string", "enum": ["automatic", "background", "foreground"]],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "required": ["delta_y"],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.scroll(
                    deltaX: server.double(arguments["delta_x"]) ?? 0,
                    deltaY: server.requireDouble(arguments["delta_y"], name: "delta_y"),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "drag",
            description: "Drag from one coordinate or element to another. Requires exactly one of two groups: (from_x, from_y, to_x, to_y) for coordinate-based dragging, or (snapshot_id, from_element_id, to_element_id) for element-based dragging. Raw coordinates require a prior query_ui snapshot so the target element can be identified and safety-checked; destructive controls and secure text fields are always blocked.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "snapshot_id": ["type": "string"],
                    "from_element_id": ["type": "string"],
                    "to_element_id": ["type": "string"],
                    "from_x": ["type": "number"],
                    "from_y": ["type": "number"],
                    "to_x": ["type": "number"],
                    "to_y": ["type": "number"],
                    "delivery_mode": ["type": "string", "enum": ["automatic", "background", "foreground"]],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "oneOf": [
                    ["required": ["from_x", "from_y", "to_x", "to_y"]],
                    ["required": ["snapshot_id", "from_element_id", "to_element_id"]],
                ],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.drag(
                    snapshotID: server.string(arguments["snapshot_id"]),
                    fromElementID: server.string(arguments["from_element_id"]),
                    toElementID: server.string(arguments["to_element_id"]),
                    fromX: server.double(arguments["from_x"]),
                    fromY: server.double(arguments["from_y"]),
                    toX: server.double(arguments["to_x"]),
                    toY: server.double(arguments["to_y"]),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "launch_app",
            description: "Launch an app by bundle_id or app_name. At least one of bundle_id or app_name is required.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "anyOf": [
                    ["required": ["bundle_id"]],
                    ["required": ["app_name"]],
                ],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.launchApp(
                    bundleID: server.string(arguments["bundle_id"]),
                    appName: server.string(arguments["app_name"]),
                    conditions: try server.actionConditions(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "focus_window",
            description: "Activate an app and optionally raise a matching window title. At least one of bundle_id or app_name is required.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "process_id": ["type": "integer"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                    "title": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "anyOf": [
                    ["required": ["bundle_id"]],
                    ["required": ["app_name"]],
                ],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.focusWindow(
                    bundleID: server.string(arguments["bundle_id"]),
                    appName: server.string(arguments["app_name"]),
                    title: server.string(arguments["title"]),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "menu_action",
            description: "Trigger a frontmost-app menu path like [\"File\", \"Save\"].",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "path": ["type": "array", "items": ["type": "string"]],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                    "precondition": server.predicateSchema(),
                    "postcondition": server.predicateSchema(),
                ],
                "required": ["path"],
            ] },
            outputSchema: { $0.actionResultSchema() },
            handler: { server, arguments in
                try server.controller.menuAction(
                    path: server.requireStringArray(arguments["path"], name: "path"),
                    conditions: try server.actionConditions(arguments),
                    target: server.targetIdentity(arguments),
                    deliveryMode: try server.deliveryMode(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "wait_for",
            description: "Wait until text appears in the frontmost UI or an app becomes available.",
            inputSchema: { _ in [
                "type": "object",
                "properties": [
                    "text": ["type": "string"],
                    "app": ["type": "string"],
                    "timeout_seconds": ["type": "number", "default": 10],
                    "process_id": ["type": "integer"],
                    "bundle_id": ["type": "string"],
                    "app_name": ["type": "string"],
                    "window_id": ["type": "integer"],
                    "window_title": ["type": "string"],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                try await server.controller.waitFor(
                    text: server.string(arguments["text"]),
                    app: server.string(arguments["app"]),
                    timeoutSeconds: server.double(arguments["timeout_seconds"]) ?? 10,
                    target: server.targetIdentity(arguments)
                )
            }
        ),
        ToolDefinition(
            name: "permissions_status",
            description: "Report screen recording and accessibility permission status.",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { server, _ in server.controller.permissionsStatus() }
        ),
        ToolDefinition(
            name: "get_policy",
            description: "Get the current action policy (deny/allow keywords, allowed bundle IDs, granted permissions).",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { server, _ in server.policyPayload() }
        ),
        ToolDefinition(
            name: "set_policy",
            description: "Update the action policy. Extends defaults; cannot weaken the built-in safety guard. Requires the policy_modify permission. When granted_permissions is provided it REPLACES the effective grant, but it may only narrow the immutable startup grant from --grant or ~/.config/symoperate/policy.json.",
            inputSchema: { server in [
                "type": "object",
                "properties": [
                    "extra_deny_keywords": ["type": "array", "items": ["type": "string"], "description": "Additional keywords to block."],
                    "allow_keywords": ["type": "array", "items": ["type": "string"], "description": "Keywords to allow (overrides deny)."],
                    "granted_permissions": ["type": "array", "items": ["type": "string"], "description": "Permission flag names to grant: capture, input, app_control, menu_action, policy_modify. Replaces the current set when present. Names that are not grantable are ignored."],
                ],
            ] },
            outputSchema: nil,
            handler: { server, arguments in
                if let violation = server.controller.actionPolicy.firstViolation(requiredPermission: .policyModify) {
                    throw AutomationError.permissionDenied(
                        "Permission denied: the '\(violation.flagNames.joined(separator: ", "))' permission is required for set_policy."
                    )
                }
                let requestedPermissions: PermissionFlags?
                if let granted = arguments["granted_permissions"] as? [String] {
                    let requested = PermissionFlags.parse(names: granted)
                    if !requested.subtracting(server.controller.actionPolicy.startupGrantedPermissions).isEmpty {
                        throw AutomationError.permissionDenied(
                            "Permission denied: set_policy cannot widen the startup grant (startup permissions: \(server.controller.actionPolicy.startupGrantedPermissions.flagNames.joined(separator: ", ")))."
                        )
                    }
                    requestedPermissions = requested
                } else {
                    requestedPermissions = nil
                }
                if let extraDeny = arguments["extra_deny_keywords"] as? [String] {
                    for kw in extraDeny { server.controller.actionPolicy.addDenyKeyword(kw) }
                }
                if let allowKw = arguments["allow_keywords"] as? [String] {
                    for kw in allowKw { server.controller.actionPolicy.allowKeyword(kw) }
                }
                if let requestedPermissions {
                    guard server.controller.actionPolicy.setGrantedPermissions(requestedPermissions) else {
                        throw AutomationError.permissionDenied("Permission denied: set_policy cannot widen the startup grant.")
                    }
                }
                return server.policyPayload()
            }
        ),
        ToolDefinition(
            name: "version",
            description: "Print current version and check for updates from GitHub releases.",
            inputSchema: { _ in [:] },
            outputSchema: nil,
            handler: { _, _ in await UpdateChecker().checkForUpdate() }
        ),
    ]

    private func tools() -> [[String: Any]] {
        Self.toolDefinitions.map { tool($0) }
    }

    private func predicateSchema() -> [String: Any] {
        [
            "type": "object",
            "additionalProperties": false,
            "properties": [
                "app": ["type": "string"],
                "window": ["type": "string"],
                "text": ["type": "string"],
                "role": ["type": "string"],
                "title": ["type": "string"],
                "label": ["type": "string"],
                "value": ["type": "string"],
                "subrole": ["type": "string"],
                "actions": ["type": "array", "items": ["type": "string"]],
                "enabled": ["type": "boolean"],
            ],
        ]
    }

    private func tool(_ definition: ToolDefinition) -> [String: Any] {
        let inputSchema = definition.inputSchema(self)
        var result: [String: Any] = [
            "name": definition.name,
            "description": definition.description,
            "inputSchema": inputSchema.isEmpty ? ["type": "object", "properties": [:]] : inputSchema,
        ]
        if let outputSchema = definition.outputSchema {
            result["outputSchema"] = outputSchema(self)
        }
        return result
    }

    private func actionResultSchema() -> [String: Any] {
        [
            "type": "object",
            "required": ["ok", "message", "contractVersion", "effect", "verification"],
            "properties": [
                "ok": ["type": "boolean"],
                "message": ["type": "string"],
                "snapshot": ["type": ["object", "null"]],
                "contractVersion": ["type": "integer", "const": EffectContract.currentVersion],
                "effect": ["type": "string", "enum": ["submitted", "confirmed", "unverifiable", "suspected_noop", "refused"]],
                "verification": [
                    "type": "object",
                    "required": ["status", "strategy"],
                    "properties": [
                        "status": ["type": "string"],
                        "strategy": ["type": "string"],
                        "reason": ["type": ["string", "null"]],
                        "snapshotID": ["type": ["string", "null"]],
                    ],
                ],
                "executionPath": ["type": ["string", "null"]],
                "deliveryMode": ["type": "string", "enum": ["automatic", "background", "foreground"]],
                "target": ["type": ["object", "null"]],
                "routeDiagnostics": ["type": ["object", "null"]],
                "conditions": ["type": ["object", "null"]],
            ],
        ]
    }

    /// Executes one tool and builds the `tools/call` result payload:
    /// `content` (human-readable JSON summary), `structuredContent`
    /// (machine-readable payload), and `isError: false`.
    private func callTool(name: String, arguments: [String: Any]) async throws -> [String: Any] {
        guard let definition = Self.toolDefinitions.first(where: { $0.name == name }) else {
            throw AutomationError.notFound("Unknown tool '\(name)'.")
        }
        let payload = try await definition.handler(self, arguments)
        let structured = try encodeToJSONObject(payload)
        let summary = textSummary(for: name, payload: structured)
        return [
            "content": [
                ["type": "text", "text": summary],
            ],
            "structuredContent": structured,
            "isError": false,
        ]
    }

    private func encodeToJSONObject(_ value: Encodable) throws -> Any {
        let boxed = AnyEncodable(value)
        let data = try encoder.encode(boxed)
        return try JSONSerialization.jsonObject(with: data)
    }

    /// Human-readable policy snapshot for get_policy/set_policy responses.
    /// `granted_permissions` is the array of granted flag names so agents can
    /// classify refusals without decoding OptionSet raw values.
    private func policyPayload() -> PolicyPayload {
        let policy = controller.actionPolicy
        return PolicyPayload(
            extraDenyKeywords: Array(policy.extraDenyKeywords).sorted(),
            allowedKeywords: Array(policy.allowedKeywords).sorted(),
            grantedPermissions: policy.grantedPermissions.flagNames
        )
    }

    private func textSummary(for tool: String, payload: Any) -> String {
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys]),
            let text = String(data: data, encoding: .utf8)
        else {
            return "\(tool) completed."
        }
        // For snapshot responses, skip the text field to avoid double base64 serialization
        if tool == "snapshot" || tool == "query_ui" || tool == "query_ui_ocr" || tool == "find_ui" {
            if let dict = payload as? [String: Any],
               dict["imageBase64PNG"] != nil {
                return "\(tool) completed. See structuredContent for full result."
            }
        }
        return text
    }

    /// Builds the JSON-RPC result payload for a classified safety refusal.
    /// Refusals are deliberately NOT JSON-RPC errors (`isError: false`) and
    /// carry a stable machine-readable `refusal.code` for agent-side
    /// classification. Every `.permissionDenied` — destructive-keyword guard
    /// or permission gate — flows through this same payload shape.
    static func refusalPayload(code: String, message: String) -> [String: Any] {
        [
            "isError": false,
            "content": [
                ["type": "text", "text": message],
            ],
            "structuredContent": [
                "status": "refused",
                "refusal": [
                    "code": code,
                    "message": message,
                ],
            ],
        ]
    }

    // MARK: - MCPJSONValue bridging
    //
    // The tool payloads above are built as `[String: Any]` (the shape the
    // in-process `dispatch` seam returns and the tests assert against).
    // These two converters bridge that shape to `SymairaMCP`'s
    // `MCPJSONValue` for the wire handlers without changing any payload
    // semantics.

    private static func jsonValue(_ value: Any) -> MCPJSONValue {
        switch value {
        case let number as NSNumber:
            // JSONSerialization produces __NSCFBoolean for JSON true/false and
            // __NSCFNumber for numbers. A plain `as? Bool` cast would also
            // match numeric 0/1 NSNumbers and corrupt integer payloads
            // (e.g. displayID 1 → true), so booleans must be discriminated
            // by their CF type. Swift-native Bool values are never NSNumbers
            // and are caught by the `as Bool` case below.
            if CFGetTypeID(number) == CFBooleanGetTypeID() {
                return .bool(number.boolValue)
            }
            return .number(number.doubleValue)
        case let bool as Bool:
            return .bool(bool)
        case let string as String:
            return .string(string)
        case let array as [Any]:
            return .array(array.map(jsonValue))
        case let object as [String: Any]:
            return .object(object.mapValues(jsonValue))
        default:
            return .null
        }
    }

    private static func jsonAny(_ value: MCPJSONValue) -> Any {
        switch value {
        case .null:
            return NSNull()
        case .bool(let bool):
            return bool
        case .number(let number):
            return number
        case .string(let string):
            return string
        case .array(let array):
            return array.map(jsonAny)
        case .object(let object):
            return object.mapValues(jsonAny)
        }
    }
}

private extension MCPServer {
    func deliveryMode(_ arguments: [String: Any]) throws -> DeliveryMode {
        guard let value = string(arguments["delivery_mode"]) else { return .automatic }
        guard let mode = DeliveryMode(rawValue: value) else {
            throw AutomationError.invalidArgument("Unsupported delivery_mode '\(value)'. Expected automatic, background, or foreground.")
        }
        return mode
    }

    func targetIdentity(_ arguments: [String: Any]) -> TargetIdentity? {
        let target = TargetIdentity(
            processID: int32(arguments["process_id"]),
            bundleID: string(arguments["bundle_id"]),
            appName: string(arguments["app_name"]),
            windowID: intOptional(arguments["window_id"]),
            windowTitle: string(arguments["window_title"]) ?? string(arguments["title"]),
            displayID: uint32(arguments["display_id"]),
            spaceID: intOptional(arguments["space_id"])
        )
        return target.isExplicit ? target : nil
    }

    func actionConditions(_ arguments: [String: Any]) throws -> ActionConditions? {
        guard arguments["precondition"] != nil || arguments["postcondition"] != nil else { return nil }
        return ActionConditions(
            precondition: try predicate(arguments["precondition"], name: "precondition"),
            postcondition: try predicate(arguments["postcondition"], name: "postcondition")
        )
    }

    func predicate(_ value: Any?, name: String) throws -> UIElementPredicate? {
        guard let value, !(value is NSNull) else { return nil }
        guard let object = value as? [String: Any] else {
            throw AutomationError.invalidArgument("\(name) must be an object predicate.")
        }
        let supported: Set<String> = [
            "app", "window", "text", "role", "title", "label", "value", "subrole", "actions", "enabled",
        ]
        let unsupported = Set(object.keys).subtracting(supported)
        guard unsupported.isEmpty else {
            throw AutomationError.invalidArgument("Unsupported \(name) field(s): \(unsupported.sorted().joined(separator: ", ")).")
        }
        func stringField(_ key: String) throws -> String? {
            guard let raw = object[key], !(raw is NSNull) else { return nil }
            guard let string = raw as? String else {
                throw AutomationError.invalidArgument("\(name).\(key) must be a string.")
            }
            return string
        }
        let actions: [String]?
        if let raw = object["actions"], !(raw is NSNull) {
            guard let parsed = raw as? [String] else {
                throw AutomationError.invalidArgument("\(name).actions must be an array of strings.")
            }
            actions = parsed
        } else {
            actions = nil
        }
        let enabled: Bool?
        if let raw = object["enabled"], !(raw is NSNull) {
            guard let parsed = raw as? Bool else {
                throw AutomationError.invalidArgument("\(name).enabled must be a boolean.")
            }
            enabled = parsed
        } else {
            enabled = nil
        }
        return UIElementPredicate(
            role: try stringField("role"),
            title: try stringField("title"),
            label: try stringField("label"),
            value: try stringField("value"),
            subrole: try stringField("subrole"),
            actions: actions,
            text: try stringField("text"),
            app: try stringField("app"),
            window: try stringField("window"),
            enabled: enabled
        )
    }

    var encoder: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }

    func string(_ value: Any?) -> String? { value as? String }

    func double(_ value: Any?) -> Double? {
        if let n = value as? NSNumber { return n.doubleValue }
        return (value as? String).flatMap(Double.init)
    }

    func uint32(_ value: Any?) -> UInt32? {
        if let n = value as? NSNumber { return n.uint32Value }
        return (value as? String).flatMap(UInt32.init)
    }

    func intOptional(_ value: Any?) -> Int? {
        if let n = value as? NSNumber { return n.intValue }
        return (value as? String).flatMap(Int.init)
    }

    func int32(_ value: Any?) -> Int32? {
        if let n = value as? NSNumber { return n.int32Value }
        return (value as? String).flatMap(Int32.init)
    }

    func int(_ value: Any?, default defaultValue: Int) -> Int {
        intOptional(value) ?? defaultValue
    }

    func bool(_ value: Any?, default defaultValue: Bool) -> Bool {
        if let b = value as? Bool { return b }
        return (value as? NSNumber)?.boolValue ?? defaultValue
    }

    func requireString(_ value: Any?, name: String) throws -> String {
        guard let string = value as? String, !string.isEmpty else {
            throw AutomationError.invalidArgument("Missing required string argument '\(name)'.")
        }
        return string
    }

    func requireDouble(_ value: Any?, name: String) throws -> Double {
        guard let double = double(value) else {
            throw AutomationError.invalidArgument("Missing required numeric argument '\(name)'.")
        }
        return double
    }

    func requireStringArray(_ value: Any?, name: String) throws -> [String] {
        guard let array = value as? [String], !array.isEmpty else {
            throw AutomationError.invalidArgument("Missing required string array argument '\(name)'.")
        }
        return array
    }
}

private struct AnyEncodable: Encodable {
    private let encodeImpl: (Encoder) throws -> Void

    init(_ wrapped: Encodable) {
        self.encodeImpl = wrapped.encode(to:)
    }

    func encode(to encoder: Encoder) throws {
        try encodeImpl(encoder)
    }
}

/// Encodable snapshot of the action policy for get_policy/set_policy responses.
/// `grantedPermissions` is exposed on the wire as `granted_permissions` — the
/// array of granted flag names in canonical order.
private struct PolicyPayload: Encodable {
    let extraDenyKeywords: [String]
    let allowedKeywords: [String]
    let grantedPermissions: [String]

    enum CodingKeys: String, CodingKey {
        case extraDenyKeywords
        case allowedKeywords
        case grantedPermissions = "granted_permissions"
    }
}
