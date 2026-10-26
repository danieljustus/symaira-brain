import Foundation
import SymOperateCore

extension MCPServer {
    /// One catalog for schema and dispatch, appended to the existing tool definitions.
    static let freeformToolDefinitions: [ToolDefinition] = [
        ToolDefinition(
            name: "freeform_doctor",
            description: "Read-only Freeform running, Accessibility and window status. Does not launch or focus an app.",
            inputSchema: { _ in [:] }, outputSchema: nil,
            handler: { server, _ in server.freeform.doctor() }
        ),
        ToolDefinition(
            name: "freeform_insert",
            description: """
                Insert validated board IR using AX menu Paste; restores all pasteboard items/types and prior PID focus.
                Positions are relative on paste, centered by Freeform. Use freeform_move for absolute placement at 100% zoom.
                AXPress is submission, not document verification. Requires input, app_control, menu_action.
                dry_run makes no system calls.
                """,
            inputSchema: { _ in insertSchema }, outputSchema: nil,
            handler: { server, arguments in try server.freeformInsert(arguments, requireConnection: false) }
        ),
        ToolDefinition(
            name: "freeform_connect",
            description: """
                Insert new objects and their anchored connections together. Same IR as freeform_insert, with at least one connection.
                Cannot attach a pasted line to an existing board object. Requires input, app_control, menu_action.
                """,
            inputSchema: { _ in insertSchema }, outputSchema: nil,
            handler: { server, arguments in try server.freeformInsert(arguments, requireConnection: true) }
        ),
        ToolDefinition(
            name: "freeform_move",
            description: """
                Move exactly one object by unique text label at 100% zoom, using a reference object with known board x/y.
                Works in background via AXSelected/AXPosition, with readback and one correction.
                Labels exclude the localized type suffix. Only visible board objects are supported.
                Requires input, app_control, menu_action.
                """,
            inputSchema: { _ in [
                "type": "object", "additionalProperties": false,
                "properties": [
                    "label": ["type": "string", "minLength": 1],
                    "reference_label": ["type": "string", "minLength": 1],
                    "reference_x": ["type": "number"], "reference_y": ["type": "number"],
                    "x": ["type": "number"], "y": ["type": "number"],
                    "zoom": ["type": "number", "const": 100], "dry_run": ["type": "boolean"],
                ],
                "required": ["label", "reference_label", "reference_x", "reference_y", "x", "y", "zoom"],
            ] }, outputSchema: nil,
            handler: { server, arguments in
                guard (arguments["zoom"] as? NSNumber)?.doubleValue == 100 else {
                    throw AutomationError.invalidArgument("freeform_move requires explicit zoom: 100.")
                }
                let request: FreeformMoveRequest = try decode(arguments)
                guard !request.label.isEmpty, !request.referenceLabel.isEmpty,
                      [request.x, request.y, request.referenceX, request.referenceY].allSatisfy(\.isFinite) else {
                    throw AutomationError.invalidArgument("Move requires nonempty labels and finite coordinates.")
                }
                if try dryRun(arguments) { return server.freeform.plan(policy: server.freeformPolicy, name: "freeform_move") }
                return try server.freeform.move(request, policy: server.freeformPolicy)
            }
        ),
        ToolDefinition(
            name: "freeform_read_board",
            description: """
                Read a caller-provided existing SQLite snapshot, never Freeform live storage.
                Optional board_identifier UUID filters board_items. First text line only;
                connection endpoints are UUID occurrence evidence, not ordered head/tail.
                Requires capture. No TCC helper in Phase 1.
                """,
            inputSchema: { _ in [
                "type": "object", "additionalProperties": false,
                "properties": ["snapshot_path": ["type": "string"], "board_identifier": ["type": "string", "format": "uuid"]],
                "required": ["snapshot_path"],
            ] }, outputSchema: nil,
            handler: { server, arguments in
                guard server.freeformPolicy.firstViolation(requiredPermission: .capture,
                      scope: PolicyScope(application: FreeformDescriptor.bundleID)) == nil else {
                    throw AutomationError.permissionDenied("freeform_read_board requires capture permission.")
                }
                guard let path = arguments["snapshot_path"] as? String else {
                    throw AutomationError.invalidArgument("snapshot_path is required.")
                }
                let identifier: UUID?
                if let raw = arguments["board_identifier"] {
                    guard let string = raw as? String, let id = UUID(uuidString: string) else {
                        throw AutomationError.invalidArgument("board_identifier must be a UUID.")
                    }
                    identifier = id
                } else { identifier = nil }
                return try FreeformBoardReader().read(snapshotPath: path, boardIdentifier: identifier)
            }
        ),
    ]

    private func freeformInsert(_ arguments: [String: Any], requireConnection: Bool) throws -> any Encodable {
        guard let value = arguments["board"] as? [String: Any] else {
            throw AutomationError.invalidArgument("board must be an IR JSON object.")
        }
        let board: FreeformBoardIR = try Self.decode(value)
        _ = try FreeformContentLanguage.build(board)
        if requireConnection && board.connections.isEmpty {
            throw AutomationError.invalidArgument("freeform_connect requires at least one connection in the board IR.")
        }
        if try Self.dryRun(arguments) {
            return freeform.plan(policy: freeformPolicy, name: requireConnection ? "freeform_connect" : "freeform_insert")
        }
        return try freeform.insert(board, policy: freeformPolicy)
    }

    private static func decode<Value: Decodable>(_ value: [String: Any]) throws -> Value {
        do { return try JSONDecoder().decode(Value.self, from: JSONSerialization.data(withJSONObject: value)) }
        catch { throw AutomationError.invalidArgument("Invalid Freeform IR: \(error.localizedDescription)") }
    }

    private static func dryRun(_ arguments: [String: Any]) throws -> Bool {
        guard let value = arguments["dry_run"] else { return false }
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
            throw AutomationError.invalidArgument("dry_run must be a boolean.")
        }
        return number.boolValue
    }

    private static var insertSchema: [String: Any] {
        let object: [String: Any] = [
            "type": "object", "additionalProperties": false,
            "properties": [
                "id": ["type": "string", "format": "uuid"],
                "kind": ["type": "string", "enum": ["shape", "text", "sticky_note"]],
                "text": ["type": "string"], "x": ["type": "number"], "y": ["type": "number"],
                "width": ["type": "number", "exclusiveMinimum": 0], "height": ["type": "number", "exclusiveMinimum": 0],
                "rgb": ["type": "array", "minItems": 3, "maxItems": 3,
                        "items": ["type": "number", "minimum": 0, "maximum": 1]],
            ],
            "required": ["id", "kind", "text", "x", "y", "width", "height", "rgb"],
        ]
        let connection: [String: Any] = [
            "type": "object", "additionalProperties": false,
            "properties": [
                "tail": ["type": "string", "format": "uuid"], "head": ["type": "string", "format": "uuid"],
                "line_end": ["type": "string", "enum": ["none"]], "line_type": ["type": "string", "enum": ["curved"]],
                "pattern": ["type": "string", "enum": ["solid"]], "magnet": ["type": "string", "enum": ["center"]],
            ],
            "required": ["tail", "head"],
        ]
        return [
            "type": "object", "additionalProperties": false,
            "properties": [
                "board": ["type": "object", "additionalProperties": false, "properties": [
                    "objects": ["type": "array", "minItems": 1, "maxItems": 500, "items": object],
                    "connections": ["type": "array", "maxItems": 1000, "items": connection],
                ], "required": ["objects"]],
                "dry_run": ["type": "boolean"],
            ],
            "required": ["board"],
        ]
    }
}
