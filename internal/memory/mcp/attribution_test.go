package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"strconv"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// helperServerCfg builds a test server with a custom config (e.g. an MCP
// client-id override set via the [mcp] config section).
func helperServerCfg(t *testing.T, cfg *config.Config) *Server {
	t.Helper()
	database := helperDB(t)
	jwtProvider, err := security.NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create JWT provider: %v", err)
	}
	return NewServer(database, jwtProvider, "test", cfg)
}

// runAttributedSession feeds a sequence of JSON-RPC request messages (framed)
// into the attributed stdio server and returns the parsed responses reordered
// to match the request order. Every message must be a request that produces
// exactly one response.
//
// Since symaira-corekit v0.12 (mcpserver.Server.ServeIO), tools/call requests
// are dispatched concurrently, so responses can land on the wire in a
// different order than the requests that produced them — matching the
// JSON-RPC spec, which correlates responses to requests by "id", not by
// position. This helper restores request order by "id" so existing
// index-based assertions (responses[i] corresponds to msgs[i]) keep working.
func runAttributedSession(t *testing.T, s *Server, msgs ...string) []map[string]interface{} {
	t.Helper()
	var input bytes.Buffer
	for _, m := range msgs {
		input.Write(frameRequest([]byte(m)))
	}
	var output bytes.Buffer
	if err := s.ServeIO(context.Background(), &input, &output); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}
	// Read all responses from a single bufio.Reader: a fresh reader per
	// response would discard bytes it read ahead into its internal buffer.
	br := bufio.NewReader(&output)
	byID := make(map[string]map[string]interface{}, len(msgs))
	for i := 0; i < len(msgs); i++ {
		resp := readFramedResponseFrom(br)
		if resp == nil {
			t.Fatalf("missing response %d of %d", i, len(msgs))
		}
		byID[fmt.Sprint(resp["id"])] = resp
	}
	responses := make([]map[string]interface{}, 0, len(msgs))
	for i, m := range msgs {
		var req struct {
			ID interface{} `json:"id"`
		}
		if err := json.Unmarshal([]byte(m), &req); err != nil {
			t.Fatalf("request %d is not valid JSON: %v", i, err)
		}
		resp, ok := byID[fmt.Sprint(req.ID)]
		if !ok {
			t.Fatalf("no response with id %v for request %d", req.ID, i)
		}
		responses = append(responses, resp)
	}
	return responses
}

// readFramedResponseFrom reads a single Content-Length framed JSON response
// from br, leaving any further responses readable from the same reader.
func readFramedResponseFrom(br *bufio.Reader) map[string]interface{} {
	var contentLength int
	for {
		line, err := br.ReadString('\n')
		if err != nil {
			return nil
		}
		line = strings.TrimRight(line, "\r\n")
		if line == "" {
			break
		}
		if rest, ok := strings.CutPrefix(line, "Content-Length:"); ok {
			n, _ := strconv.Atoi(strings.TrimSpace(rest))
			contentLength = n
		}
	}
	if contentLength <= 0 {
		return nil
	}
	body := make([]byte, contentLength)
	if _, err := io.ReadFull(br, body); err != nil {
		return nil
	}
	var result map[string]interface{}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil
	}
	return result
}

const testInitializeMsg = `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"claude-desktop","version":"1.2.3"}}}`

const testInitializeMsgWithInstance = `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"cursor","version":"1.0.0","clientId":"host-abc"}}}`

const testInitializeMsgNoClientInfo = `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}`

func toolCallMsg(id int, name string, args map[string]interface{}) string {
	params, _ := json.Marshal(map[string]interface{}{"name": name, "arguments": args})
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":%s}`, id, params)
}

func memoryIDFromSetText(t *testing.T, text string) string {
	t.Helper()
	// Live wording: "Memory saved successfully with ID: <uuid>"
	const prefix = "Memory saved successfully with ID: "
	if strings.HasPrefix(text, prefix) {
		return strings.TrimSpace(strings.TrimPrefix(text, prefix))
	}
	// Staged wording (#485): "Memory staged as candidate (not yet retrievable) with ID: <uuid>. ..."
	const stagedPrefix = "Memory staged as candidate (not yet retrievable) with ID: "
	if strings.HasPrefix(text, stagedPrefix) {
		rest := strings.TrimPrefix(text, stagedPrefix)
		if j := strings.IndexAny(rest, " ."); j >= 0 {
			return rest[:j]
		}
	}
	t.Fatalf("unexpected memory_set response: %q", text)
	return ""
}

// TestMCPServeAttribution_HandshakeIdentity verifies that a client which
// sends clientInfo in the initialize handshake gets attributed as the author
// of memory_set writes instead of the literal actor "mcp".
func TestMCPServeAttribution_HandshakeIdentity(t *testing.T) {
	s := helperServer(t)

	responses := runAttributedSession(t, s,
		testInitializeMsg,
		toolCallMsg(2, "memory_set", map[string]interface{}{"content": "attribute me", "kind": "user", "scope": "global"}),
	)
	if code, msg := getToolError(responses[1]); code != 0 {
		t.Fatalf("unexpected memory_set error: %v %s", code, msg)
	}
	id := memoryIDFromSetText(t, getToolText(responses[1]))

	m, err := s.service.Get(id)
	if err != nil {
		t.Fatalf("Get(%s): %v", id, err)
	}
	if m.CreatedBy != "claude-desktop/1.2.3" {
		t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "claude-desktop/1.2.3")
	}
	if m.CreatedSession != "" {
		t.Errorf("CreatedSession = %q, want empty (no session_id passed)", m.CreatedSession)
	}
}

// TestMCPServeAttribution_InstanceID verifies a stable per-host instance id
// sent inside clientInfo is folded into the attribution identity.
func TestMCPServeAttribution_InstanceID(t *testing.T) {
	s := helperServer(t)

	responses := runAttributedSession(t, s,
		testInitializeMsgWithInstance,
		toolCallMsg(2, "memory_set", map[string]interface{}{"content": "attributed with instance", "kind": "user", "scope": "global"}),
	)
	if code, msg := getToolError(responses[1]); code != 0 {
		t.Fatalf("unexpected memory_set error: %v %s", code, msg)
	}
	id := memoryIDFromSetText(t, getToolText(responses[1]))

	m, err := s.service.Get(id)
	if err != nil {
		t.Fatalf("Get(%s): %v", id, err)
	}
	if m.CreatedBy != "cursor/1.0.0#host-abc" {
		t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "cursor/1.0.0#host-abc")
	}
}

// TestMCPServeAttribution_ConfigOverrideWins verifies that an explicit
// [mcp] client_id config value wins over the handshake-derived identity.
func TestMCPServeAttribution_ConfigOverrideWins(t *testing.T) {
	cfg := config.Defaults()
	cfg.MCP.ClientID = "configured-client"
	s := helperServerCfg(t, cfg)

	responses := runAttributedSession(t, s,
		testInitializeMsg,
		toolCallMsg(2, "memory_set", map[string]interface{}{"content": "override me", "kind": "user", "scope": "global"}),
	)
	if code, msg := getToolError(responses[1]); code != 0 {
		t.Fatalf("unexpected memory_set error: %v %s", code, msg)
	}
	id := memoryIDFromSetText(t, getToolText(responses[1]))

	m, err := s.service.Get(id)
	if err != nil {
		t.Fatalf("Get(%s): %v", id, err)
	}
	if m.CreatedBy != "configured-client" {
		t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "configured-client")
	}
}

// TestMCPServeAttribution_FlagOverrideWins verifies that the serve --client-id
// override (SetClientIDOverride) wins over both the config value and the
// handshake identity.
func TestMCPServeAttribution_FlagOverrideWins(t *testing.T) {
	cfg := config.Defaults()
	cfg.MCP.ClientID = "configured-client"
	s := helperServerCfg(t, cfg)
	s.SetClientIDOverride("flag-client")

	responses := runAttributedSession(t, s,
		testInitializeMsg,
		toolCallMsg(2, "memory_set", map[string]interface{}{"content": "flag wins", "kind": "user", "scope": "global"}),
	)
	if code, msg := getToolError(responses[1]); code != 0 {
		t.Fatalf("unexpected memory_set error: %v %s", code, msg)
	}
	id := memoryIDFromSetText(t, getToolText(responses[1]))

	m, err := s.service.Get(id)
	if err != nil {
		t.Fatalf("Get(%s): %v", id, err)
	}
	if m.CreatedBy != "flag-client" {
		t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "flag-client")
	}
}

// TestMCPServeAttribution_Fallback verifies that writes fall back to the
// literal actor "mcp" when no identity is resolvable: neither an initialize
// handshake carrying clientInfo nor an explicit override.
func TestMCPServeAttribution_Fallback(t *testing.T) {
	t.Run("initialize without clientInfo", func(t *testing.T) {
		s := helperServer(t)
		responses := runAttributedSession(t, s,
			testInitializeMsgNoClientInfo,
			toolCallMsg(2, "memory_set", map[string]interface{}{"content": "fallback one", "kind": "user", "scope": "global"}),
		)
		if code, msg := getToolError(responses[1]); code != 0 {
			t.Fatalf("unexpected memory_set error: %v %s", code, msg)
		}
		id := memoryIDFromSetText(t, getToolText(responses[1]))
		m, err := s.service.Get(id)
		if err != nil {
			t.Fatalf("Get(%s): %v", id, err)
		}
		if m.CreatedBy != "mcp" {
			t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "mcp")
		}
	})

	t.Run("no initialize before tools/call", func(t *testing.T) {
		s := helperServer(t)
		responses := runAttributedSession(t, s,
			toolCallMsg(1, "memory_set", map[string]interface{}{"content": "fallback two", "kind": "user", "scope": "global"}),
		)
		if code, msg := getToolError(responses[0]); code != 0 {
			t.Fatalf("unexpected memory_set error: %v %s", code, msg)
		}
		id := memoryIDFromSetText(t, getToolText(responses[0]))
		m, err := s.service.Get(id)
		if err != nil {
			t.Fatalf("Get(%s): %v", id, err)
		}
		if m.CreatedBy != "mcp" {
			t.Errorf("CreatedBy = %q, want %q", m.CreatedBy, "mcp")
		}
	})
}

// TestMCPServeAttribution_EntityRelate verifies that both entity_relate write
// paths persist the handshake-derived identity in created_by.
func TestMCPServeAttribution_EntityRelate(t *testing.T) {
	s := helperServer(t)
	if err := s.service.db.SaveEntity(&db.Entity{ID: "attr-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "attr-bob", Name: "Bob", Type: "person"}); err != nil {
		t.Fatalf("seed Bob: %v", err)
	}

	responses := runAttributedSession(t, s,
		testInitializeMsg,
		toolCallMsg(2, "entity_relate", map[string]interface{}{
			"from": "Alice", "to": "Bob", "relation": "works-with",
		}),
		toolCallMsg(3, "entity_relate", map[string]interface{}{
			"from_id": "attr-alice", "to_id": "attr-bob", "relation": "mentors",
			"source": "symdesk", "source_ref": "doc-42",
		}),
	)
	if code, msg := getToolError(responses[1]); code != 0 {
		t.Fatalf("unexpected entity_relate error: %v %s", code, msg)
	}
	if code, msg := getToolError(responses[2]); code != 0 {
		t.Fatalf("unexpected entity_relate (provenance) error: %v %s", code, msg)
	}

	// Plain create path.
	out, err := s.service.db.OutgoingRelations("attr-alice")
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	var plainRel *db.EntityRelation
	for i := range out {
		if out[i].RelationType == "works-with" {
			plainRel = out[i]
		}
	}
	if plainRel == nil {
		t.Fatalf("expected works-with relation, got %+v", out)
	}
	if plainRel.CreatedBy != "claude-desktop/1.2.3" {
		t.Errorf("plain path CreatedBy = %q, want %q", plainRel.CreatedBy, "claude-desktop/1.2.3")
	}

	// Provenance path (returns the saved relation as JSON).
	var saved db.EntityRelation
	if err := json.Unmarshal([]byte(getToolText(responses[2])), &saved); err != nil {
		t.Fatalf("provenance response is not JSON: %v", err)
	}
	if saved.CreatedBy != "claude-desktop/1.2.3" {
		t.Errorf("provenance path CreatedBy = %q, want %q", saved.CreatedBy, "claude-desktop/1.2.3")
	}
	fetched, err := s.service.db.GetEntityRelationByID(saved.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID: %v", err)
	}
	if fetched == nil || fetched.CreatedBy != "claude-desktop/1.2.3" {
		t.Errorf("persisted provenance CreatedBy = %v, want %q", fetched, "claude-desktop/1.2.3")
	}
}

// TestMCPServeAttribution_ToolsListAfterInitialize verifies that intercepting
// the initialize handshake for attribution does not disturb the rest of the
// protocol: the initialize response is still produced by the underlying
// server and tools/list keeps working.
func TestMCPServeAttribution_ToolsListAfterInitialize(t *testing.T) {
	s := helperServer(t)
	responses := runAttributedSession(t, s,
		testInitializeMsg,
		`{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}`,
	)

	initResult, ok := responses[0]["result"].(map[string]interface{})
	if !ok {
		t.Fatalf("initialize response missing result: %v", responses[0])
	}
	serverInfo, ok := initResult["serverInfo"].(map[string]interface{})
	if !ok || serverInfo["name"] != "symaira-memory" {
		t.Errorf("unexpected initialize serverInfo: %v", initResult["serverInfo"])
	}

	listResult, ok := responses[1]["result"].(map[string]interface{})
	if !ok {
		t.Fatalf("tools/list response missing result: %v", responses[1])
	}
	tools, ok := listResult["tools"].([]interface{})
	if !ok {
		t.Fatalf("tools/list result has no tools array: %v", listResult)
	}
	found := false
	for _, tl := range tools {
		tm, ok := tl.(map[string]interface{})
		if ok && tm["name"] == "memory_set" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("tools/list after attributed initialize does not include memory_set: %v", tools)
	}
}

// Unit-level coverage for the identity helpers.
func TestParseClientInfo(t *testing.T) {
	tests := []struct {
		name   string
		params string
		want   ClientIdentity
	}{
		{"empty params", "", ClientIdentity{}},
		{"no clientInfo", `{"protocolVersion":"2024-11-05"}`, ClientIdentity{}},
		{"name and version", `{"clientInfo":{"name":"claude","version":"2.0"}}`, ClientIdentity{Name: "claude", Version: "2.0"}},
		{"name only", `{"clientInfo":{"name":"claude"}}`, ClientIdentity{Name: "claude"}},
		{"version only is not an identity", `{"clientInfo":{"version":"2.0"}}`, ClientIdentity{}},
		{"clientId instance", `{"clientInfo":{"name":"cursor","version":"1.0","clientId":"h1"}}`, ClientIdentity{Name: "cursor", Version: "1.0", InstanceID: "h1"}},
		{"instanceId field", `{"clientInfo":{"name":"zed","version":"1","instanceId":"h2"}}`, ClientIdentity{Name: "zed", Version: "1", InstanceID: "h2"}},
		{"malformed", `{not json`, ClientIdentity{}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := parseClientInfo(json.RawMessage(tt.params))
			if got != tt.want {
				t.Errorf("parseClientInfo(%q) = %+v, want %+v", tt.params, got, tt.want)
			}
		})
	}
}

func TestClientIdentityString(t *testing.T) {
	tests := []struct {
		id   ClientIdentity
		want string
	}{
		{ClientIdentity{}, ""},
		{ClientIdentity{Name: "claude"}, "claude"},
		{ClientIdentity{Name: "claude", Version: "1.0"}, "claude/1.0"},
		{ClientIdentity{Name: "claude", Version: "1.0", InstanceID: "h1"}, "claude/1.0#h1"},
		{ClientIdentity{Name: "claude", InstanceID: "h1"}, "claude#h1"},
	}
	for _, tt := range tests {
		if got := tt.id.String(); got != tt.want {
			t.Errorf("(%+v).String() = %q, want %q", tt.id, got, tt.want)
		}
	}
}

func TestAttributionActorPrecedence(t *testing.T) {
	s := helperServer(t)
	if got := s.attributionActor(); got != "mcp" {
		t.Fatalf("default actor = %q, want %q", got, "mcp")
	}

	s.setHandshakeIdentity(ClientIdentity{Name: "claude", Version: "1.0"})
	if got := s.attributionActor(); got != "claude/1.0" {
		t.Fatalf("handshake actor = %q, want %q", got, "claude/1.0")
	}

	s.SetClientIDOverride("override")
	if got := s.attributionActor(); got != "override" {
		t.Fatalf("override actor = %q, want %q", got, "override")
	}
}
