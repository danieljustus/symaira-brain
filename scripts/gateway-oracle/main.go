// Command gateway-oracle freezes the first language-neutral MCP gateway seam.
// It drives the production Go gateway with a checked-in fake MCP child; no
// installed Symaira binary or live service is involved.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/gateway"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

const profileTOML = `[profile]
name = "gateway-fixture"
description = "deterministic gateway oracle"

[servers.vault]
enabled = true
mode = "full"
tools_deny = ["hidden"]

[audit]
enabled = false
`

type childTool struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	InputSchema json.RawMessage `json:"input_schema,omitempty"`
	Behavior    string          `json:"behavior,omitempty"`
}

type request struct {
	JSONRPC string         `json:"jsonrpc"`
	ID      int            `json:"id"`
	Method  string         `json:"method"`
	Params  map[string]any `json:"params,omitempty"`
}

type errorExpectation struct {
	RequestID int    `json:"request_id"`
	Category  string `json:"category"`
	Retryable bool   `json:"retryable"`
}

type caseExpectation struct {
	ID                   string             `json:"id"`
	ChildBehavior        map[string]string  `json:"child_behavior,omitempty"`
	Requests             []request          `json:"requests"`
	Stdout               string             `json:"stdout"`
	StdoutOnlyJSONRPC    bool               `json:"stdout_only_jsonrpc"`
	StdoutNonProtocol    int                `json:"stdout_non_protocol_bytes"`
	ErrorClassifications []errorExpectation `json:"error_classifications,omitempty"`
}

type fixture struct {
	SchemaVersion int               `json:"schema_version"`
	ProfileTOML   string            `json:"profile_toml"`
	ChildTools    []childTool       `json:"child_tools"`
	Cases         []caseExpectation `json:"cases"`
}

func tools(toolError bool) []childTool {
	requestCredential := childTool{Name: "request_credential", Description: "request a credential"}
	if toolError {
		requestCredential.Behavior = "toolerror"
	}
	return []childTool{
		{Name: "health", Description: "health check", InputSchema: json.RawMessage(`{"type":"object","properties":{"probe":{"type":"string"}}}`)},
		{Name: "get_entry", Description: "fetch secret", InputSchema: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string"}}}`)},
		requestCredential,
		{Name: "hidden", Description: "policy-hidden tool"},
	}
}

func initializeRequest(id int) request {
	return request{JSONRPC: "2.0", ID: id, Method: "initialize", Params: map[string]any{
		"protocolVersion": "2024-11-05",
		"capabilities":    map[string]any{},
		"clientInfo":      map[string]any{"name": "gateway-fixture", "version": "0"},
	}}
}

func listRequest(id int) request {
	return request{JSONRPC: "2.0", ID: id, Method: "tools/list", Params: map[string]any{}}
}

func callRequest(id int, name string, args map[string]any) request {
	return request{JSONRPC: "2.0", ID: id, Method: "tools/call", Params: map[string]any{
		"name": name, "arguments": args,
	}}
}

func runCase(root, fakePath string, requests []request, childTools []childTool) (caseExpectation, error) {
	home, err := os.MkdirTemp(root, "gateway-home-")
	if err != nil {
		return caseExpectation{}, err
	}
	defer os.RemoveAll(home)

	configDir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		return caseExpectation{}, err
	}
	if err := os.WriteFile(filepath.Join(configDir, "gateway-fixture.toml"), []byte(profileTOML), 0o600); err != nil {
		return caseExpectation{}, err
	}

	oldHome, oldConfig := os.Getenv("HOME"), os.Getenv("XDG_CONFIG_HOME")
	defer func() { _ = os.Setenv("HOME", oldHome); _ = os.Setenv("XDG_CONFIG_HOME", oldConfig) }()
	if err := os.Setenv("HOME", home); err != nil {
		return caseExpectation{}, err
	}
	if err := os.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config")); err != nil {
		return caseExpectation{}, err
	}
	p, err := profile.Load("gateway-fixture")
	if err != nil {
		return caseExpectation{}, fmt.Errorf("load fixture profile: %w", err)
	}

	toolJSON, err := json.Marshal(childTools)
	if err != nil {
		return caseExpectation{}, err
	}
	discard := slog.New(slog.NewTextHandler(io.Discard, nil))
	ms := broker.NewManagedServer(broker.ServerConfig{
		Name:            "vault",
		BinaryPath:      fakePath,
		Env:             []string{"FAKEMCP_TOOLS=" + string(toolJSON)},
		InitTimeout:     2 * time.Second,
		CallTimeout:     2 * time.Second,
		ShutdownTimeout: 2 * time.Second,
		Logger:          discard,
	})
	defer ms.Shutdown()

	server := gateway.New(p, map[string]*broker.ManagedServer{"vault": ms}, discard, nil, "dev")
	var input bytes.Buffer
	for _, req := range requests {
		data, err := json.Marshal(req)
		if err != nil {
			return caseExpectation{}, err
		}
		input.Write(data)
		input.WriteByte('\n')
	}
	var output bytes.Buffer
	if err := server.ServeIO(context.Background(), &input, &output); err != nil {
		return caseExpectation{}, fmt.Errorf("serve fixture session: %w", err)
	}
	normalizedOutput := normalizeRuntimeOutput(output.String())

	frames, valid, nonProtocol, err := parseFrames([]byte(normalizedOutput))
	if err != nil {
		return caseExpectation{}, err
	}
	childBehavior := make(map[string]string)
	for _, tool := range childTools {
		if tool.Behavior != "" {
			childBehavior[tool.Name] = tool.Behavior
		}
	}
	if len(childBehavior) == 0 {
		childBehavior = nil
	}
	result := caseExpectation{
		ChildBehavior:     childBehavior,
		Requests:          requests,
		Stdout:            normalizedOutput,
		StdoutOnlyJSONRPC: valid,
		StdoutNonProtocol: nonProtocol,
	}
	for _, frame := range frames {
		var envelope struct {
			ID     int `json:"id"`
			Result *struct {
				IsError bool `json:"isError"`
			} `json:"result,omitempty"`
			Error *struct {
				Code int `json:"code"`
			} `json:"error,omitempty"`
		}
		if err := json.Unmarshal(frame, &envelope); err != nil {
			return caseExpectation{}, err
		}
		for _, req := range requests {
			if req.ID != envelope.ID || req.Method != "tools/call" {
				continue
			}
			name, _ := req.Params["name"].(string)
			category := "rpc"
			if name == "vault_request_credential" {
				category = "tool"
			}
			if envelope.Result != nil && envelope.Result.IsError {
				category = "tool"
			}
			if (envelope.Result != nil && envelope.Result.IsError) || envelope.Error != nil {
				result.ErrorClassifications = append(result.ErrorClassifications, errorExpectation{RequestID: envelope.ID, Category: category})
			}
			break
		}
	}
	return result, nil
}

func normalizeRuntimeOutput(output string) string {
	const marker = `\"generated_at\":\"`
	start := strings.Index(output, marker)
	if start < 0 {
		return output
	}
	valueStart := start + len(marker)
	valueEnd := strings.Index(output[valueStart:], `\"`)
	if valueEnd < 0 {
		return output
	}
	valueEnd += valueStart
	return output[:valueStart] + "<runtime>" + output[valueEnd:]
}

func parseFrames(data []byte) ([]json.RawMessage, bool, int, error) {
	lines := bytes.Split(data, []byte{'\n'})
	frames := make([]json.RawMessage, 0, len(lines))
	nonProtocol := 0
	for _, line := range lines {
		if len(line) == 0 {
			continue
		}
		var envelope struct {
			JSONRPC string `json:"jsonrpc"`
		}
		if err := json.Unmarshal(line, &envelope); err != nil || envelope.JSONRPC != "2.0" {
			nonProtocol += len(line)
			continue
		}
		frames = append(frames, json.RawMessage(append([]byte(nil), line...)))
	}
	if nonProtocol > 0 {
		return frames, false, nonProtocol, fmt.Errorf("gateway stdout contains %d non-JSON-RPC bytes", nonProtocol)
	}
	return frames, true, 0, nil
}

func buildFake(root string) (string, func(), error) {
	dir, err := os.MkdirTemp("", "symbrain-gateway-fake-")
	if err != nil {
		return "", func() {}, err
	}
	cleanup := func() { _ = os.RemoveAll(dir) }
	path := filepath.Join(dir, "fakemcp")
	cmd := exec.Command("go", "build", "-o", path, "./internal/broker/testdata/fakemcp")
	cmd.Dir = root
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		cleanup()
		return "", func() {}, fmt.Errorf("build fake MCP child: %w\n%s", err, output)
	}
	return path, cleanup, nil
}

func generate(root string) (fixture, error) {
	fakePath, cleanup, err := buildFake(root)
	if err != nil {
		return fixture{}, err
	}
	defer cleanup()
	baseTools := tools(false)
	errorTools := tools(true)
	cases := []struct {
		id       string
		requests []request
		tools    []childTool
	}{
		{"initialize-capabilities", []request{initializeRequest(1)}, baseTools},
		{"tools-list-catalog", []request{initializeRequest(1), listRequest(2)}, baseTools},
		{"tools-call-routing", []request{
			initializeRequest(1),
			callRequest(2, "vault_health", map[string]any{"probe": "gateway"}),
		}, baseTools},
		{"tools-call-tool-error", []request{
			initializeRequest(1),
			callRequest(2, "vault_request_credential", map[string]any{}),
		}, errorTools},
		{"tools-call-hidden", []request{
			initializeRequest(1),
			callRequest(2, "vault_hidden", map[string]any{}),
		}, errorTools},
		{"tools-call-bootstrap", []request{
			initializeRequest(1),
			callRequest(2, "bootstrap", map[string]any{}),
		}, baseTools},
		{"tools-call-patterns", []request{
			initializeRequest(1),
			callRequest(2, "patterns", map[string]any{}),
		}, baseTools},
		{"stdout-hygiene", []request{
			initializeRequest(1), listRequest(2),
			callRequest(3, "vault_get_entry", map[string]any{"id": "fixture"}),
		}, baseTools},
	}
	result := fixture{SchemaVersion: 1, ProfileTOML: profileTOML, ChildTools: baseTools}
	for _, tc := range cases {
		got, err := runCase(root, fakePath, tc.requests, tc.tools)
		if err != nil {
			return fixture{}, fmt.Errorf("case %s: %w", tc.id, err)
		}
		got.ID = tc.id
		result.Cases = append(result.Cases, got)
	}
	return result, nil
}

func main() {
	check := flag.Bool("check", false, "fail if the generated fixture differs")
	output := flag.String("output", "rust/symbrain-gateway/tests/fixtures/gateway_cases.json", "fixture path")
	flag.Parse()
	root, err := os.Getwd()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	generated, err := generate(root)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	data, err := json.MarshalIndent(generated, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/gateway-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: gateway oracle deterministic check passed (%d cases)\n", len(generated.Cases))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(generated.Cases))
}
