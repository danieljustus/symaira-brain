import Foundation

/// The single source of truth for an Operate MCP tool's public metadata and
/// implementation. The catalog is kept on `MCPServer` so tools/list and
/// tools/call cannot acquire different names or definitions.
struct ToolDefinition: @unchecked Sendable {
    let name: String
    let description: String
    let inputSchema: (MCPServer) -> [String: Any]
    let outputSchema: ((MCPServer) -> [String: Any])?
    let handler: (MCPServer, [String: Any]) async throws -> Encodable
}
