package broker

import "encoding/json"

// protocolVersion is the MCP protocol version symbrain's broker negotiates
// during initialize. It matches corekit/mcpserver's ProtocolVersion so a
// symbrain-gateway-fronted child and a corekit-based child agree by
// construction.
const protocolVersion = "2024-11-05"

// clientName/clientVersion identify symbrain to child servers during the
// initialize handshake's clientInfo field.
const clientName = "symbrain"

// rpcRequest is the wire shape of an outgoing JSON-RPC 2.0 request or
// notification. ID is a pointer so notifications (no response expected) can
// omit the field entirely via json:",omitempty" rather than sending a
// misleading `"id":null`.
type rpcRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *int64          `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

// rpcResponse is the wire shape of an incoming JSON-RPC 2.0 response.
type rpcResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *int64          `json:"id"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *rpcErrorObject `json:"error,omitempty"`
}

type rpcErrorObject struct {
	Code    int             `json:"code"`
	Message string          `json:"message"`
	Data    json.RawMessage `json:"data,omitempty"`
}

// initializeParams is what the broker sends as the "params" of an
// initialize request.
type initializeParams struct {
	ProtocolVersion string         `json:"protocolVersion"`
	Capabilities    map[string]any `json:"capabilities"`
	ClientInfo      clientInfo     `json:"clientInfo"`
}

type clientInfo struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// InitializeResult is the parsed result of a successful initialize call.
type InitializeResult struct {
	ProtocolVersion string          `json:"protocolVersion"`
	ServerInfo      ServerInfo      `json:"serverInfo"`
	Capabilities    json.RawMessage `json:"capabilities,omitempty"`
	Instructions    string          `json:"instructions,omitempty"`
}

// ServerInfo mirrors the MCP "serverInfo" object a child returns in its
// initialize response.
type ServerInfo struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// Tool describes one tool a child server exposes, as reported by its
// tools/list response. Description and InputSchema are kept as the child
// sent them; the broker does not interpret or modify either (see
// internal/catalog, which passes both through unmodified downstream too).
type Tool struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	InputSchema json.RawMessage `json:"inputSchema,omitempty"`
}

type toolsListResult struct {
	Tools []Tool `json:"tools"`
}

type toolsCallParams struct {
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments,omitempty"`
}

// ContentBlock is one entry of a tools/call result's "content" array. Only
// the "text" type is modeled: every symbrain child (vault/memory/skills)
// only ever returns text content today. A future content type still
// round-trips through Tool/CallToolResult call sites that only read Text,
// so this is a deliberate, documented simplification rather than an
// oversight.
type ContentBlock struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

// CallToolResult is the parsed result of a successful (JSON-RPC level)
// tools/call. IsError distinguishes a *tool-level* failure (e.g. the vault
// entry was not found) from a JSON-RPC-level error (RPCError), matching the
// MCP spec's split between transport errors and tool errors.
type CallToolResult struct {
	Content []ContentBlock `json:"content"`
	IsError bool           `json:"isError"`
}
