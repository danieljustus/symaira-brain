// Command fakemcp is a test-only MCP server speaking JSON-RPC 2.0 over
// stdio (newline-delimited JSON, matching internal/broker's client). It
// exists so internal/broker's integration tests never need a real Symaira
// binary on PATH (CI containers have none — see AGENTS.md "Integration
// tests against the broker use a fake MCP child binary").
//
// This file lives under testdata/ so the Go toolchain's "..." pattern
// (go build ./..., go vet ./...) skips it; the broker package's tests
// build it explicitly via `go build ./testdata/fakemcp` (see
// integration_test.go).
//
// Configuration is via environment variables, set by the spawning test:
//
//   - FAKEMCP_TOOLS: a JSON array of tool definitions, each
//     {"name","description","input_schema","behavior"}. behavior selects
//     the tools/call handling: "echo" (default) echoes the call arguments
//     back as the result text; "crash" exits the process immediately
//     without responding; "slow" sleeps FAKEMCP_SLOW_MS before responding
//     like "echo"; "toolerror" responds with isError:true. When unset, a
//     default tool set covering all four behaviors is registered.
//   - FAKEMCP_SLOW_MS: milliseconds a "slow" tool sleeps before
//     responding (default 2000).
//   - FAKEMCP_INIT_DELAY_MS: milliseconds to delay the initialize
//     response by (default 0), for handshake-timeout tests.
//   - FAKEMCP_VERSION: the version string reported by both
//     `fakemcp version --json` and the initialize serverInfo (default
//     "0.0.0-fakemcp").
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strconv"
	"time"
)

type toolDef struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	InputSchema json.RawMessage `json:"input_schema"`
	Behavior    string          `json:"behavior"`
}

func defaultTools() []toolDef {
	return []toolDef{
		{Name: "echo", Description: "echoes call arguments back", Behavior: "echo"},
		{Name: "crash", Description: "exits the process without responding", Behavior: "crash"},
		{Name: "slow", Description: "sleeps before responding", Behavior: "slow"},
		{Name: "toolerror", Description: "always returns a tool-level error", Behavior: "toolerror"},
	}
}

func main() {
	if len(os.Args) >= 3 && os.Args[1] == "version" && os.Args[2] == "--json" {
		fmt.Println(`{"version":"` + version() + `"}`)
		return
	}
	serve(os.Stdin, os.Stdout)
}

func version() string {
	if v := os.Getenv("FAKEMCP_VERSION"); v != "" {
		return v
	}
	return "0.0.0-fakemcp"
}

func tools() []toolDef {
	raw := os.Getenv("FAKEMCP_TOOLS")
	if raw == "" {
		return defaultTools()
	}
	var defs []toolDef
	if err := json.Unmarshal([]byte(raw), &defs); err != nil {
		fmt.Fprintf(os.Stderr, "fakemcp: invalid FAKEMCP_TOOLS: %v\n", err)
		os.Exit(2)
	}
	return defs
}

func envMillis(name string, fallback int) time.Duration {
	v := os.Getenv(name)
	if v == "" {
		return time.Duration(fallback) * time.Millisecond
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return time.Duration(fallback) * time.Millisecond
	}
	return time.Duration(n) * time.Millisecond
}

type rpcRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *int64          `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

func serve(r io.Reader, w io.Writer) {
	defs := tools()
	byName := make(map[string]toolDef, len(defs))
	for _, d := range defs {
		byName[d.Name] = d
	}

	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var req rpcRequest
		if err := json.Unmarshal(line, &req); err != nil {
			continue // not a protocol conformance checker
		}
		if req.ID == nil {
			continue // notification: nothing to respond to
		}
		handle(w, defs, byName, req)
	}
}

func handle(w io.Writer, defs []toolDef, byName map[string]toolDef, req rpcRequest) {
	switch req.Method {
	case "initialize":
		if d := envMillis("FAKEMCP_INIT_DELAY_MS", 0); d > 0 {
			time.Sleep(d)
		}
		writeResult(w, *req.ID, map[string]any{
			"protocolVersion": "2024-11-05",
			"capabilities":    map[string]any{"tools": map[string]any{}},
			"serverInfo":      map[string]string{"name": "fakemcp", "version": version()},
		})
	case "tools/list":
		list := make([]map[string]any, 0, len(defs))
		for _, d := range defs {
			entry := map[string]any{"name": d.Name, "description": d.Description}
			if len(d.InputSchema) > 0 {
				var schema any
				if err := json.Unmarshal(d.InputSchema, &schema); err == nil {
					entry["inputSchema"] = schema
				}
			}
			list = append(list, entry)
		}
		writeResult(w, *req.ID, map[string]any{"tools": list})
	case "tools/call":
		handleToolsCall(w, byName, req)
	default:
		writeError(w, *req.ID, -32601, "Method not found: "+req.Method)
	}
}

func handleToolsCall(w io.Writer, byName map[string]toolDef, req rpcRequest) {
	var params struct {
		Name      string          `json:"name"`
		Arguments json.RawMessage `json:"arguments"`
	}
	if err := json.Unmarshal(req.Params, &params); err != nil {
		writeError(w, *req.ID, -32602, "Invalid params: "+err.Error())
		return
	}

	d, ok := byName[params.Name]
	if !ok {
		writeError(w, *req.ID, -32601, "Unknown tool: "+params.Name)
		return
	}

	switch d.Behavior {
	case "crash":
		os.Exit(1)
	case "slow":
		time.Sleep(envMillis("FAKEMCP_SLOW_MS", 2000))
		writeToolText(w, *req.ID, string(params.Arguments), false)
	case "toolerror":
		writeToolText(w, *req.ID, "toolerror: intentional failure for "+params.Name, true)
	default: // "echo" and anything unrecognized
		text := string(params.Arguments)
		if text == "" {
			text = "{}"
		}
		writeToolText(w, *req.ID, text, false)
	}
}

func writeResult(w io.Writer, id int64, result any) {
	writeLine(w, map[string]any{"jsonrpc": "2.0", "id": id, "result": result})
}

func writeToolText(w io.Writer, id int64, text string, isError bool) {
	writeResult(w, id, map[string]any{
		"content": []map[string]any{{"type": "text", "text": text}},
		"isError": isError,
	})
}

func writeError(w io.Writer, id int64, code int, message string) {
	writeLine(w, map[string]any{
		"jsonrpc": "2.0",
		"id":      id,
		"error":   map[string]any{"code": code, "message": message},
	})
}

func writeLine(w io.Writer, v any) {
	data, err := json.Marshal(v)
	if err != nil {
		fmt.Fprintf(os.Stderr, "fakemcp: marshal response: %v\n", err)
		return
	}
	data = append(data, '\n')
	_, _ = w.Write(data)
}
