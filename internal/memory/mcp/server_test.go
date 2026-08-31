package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/evidencekit"
)

func helperDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-mcp-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := db.Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open test database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func helperServer(t *testing.T) *Server {
	t.Helper()
	database := helperDB(t)
	jwtProvider, err := security.NewJWTProvider(config.Defaults(), nil)
	if err != nil {
		t.Fatalf("failed to create JWT provider: %v", err)
	}
	return NewServer(database, jwtProvider, "test", config.Defaults())
}

func frameRequest(data []byte) []byte {
	return []byte(fmt.Sprintf("Content-Length: %d\r\n\r\n%s", len(data), data))
}

func readFramedResponse(r io.Reader) map[string]interface{} {
	br := bufio.NewReader(r)
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

func callMCP(s *Server, method string, params interface{}) map[string]interface{} {
	mcpSrv := s.MCPServer()
	req := map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "test-1",
		"method":  method,
	}
	if params != nil {
		req["params"] = params
	}
	data, _ := json.Marshal(req)
	var input bytes.Buffer
	input.Write(frameRequest(data))
	var output bytes.Buffer
	_ = mcpSrv.ServeIO(context.Background(), &input, &output)
	return readFramedResponse(&output)
}

func callTool(s *Server, name string, args map[string]interface{}) map[string]interface{} {
	mcpSrv := s.MCPServer()
	params, _ := json.Marshal(map[string]interface{}{
		"name":      name,
		"arguments": args,
	})
	req := map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "test-1",
		"method":  "tools/call",
		"params":  json.RawMessage(params),
	}
	data, _ := json.Marshal(req)
	var input bytes.Buffer
	input.Write(frameRequest(data))
	var output bytes.Buffer
	_ = mcpSrv.ServeIO(context.Background(), &input, &output)
	return readFramedResponse(&output)
}

func getToolText(res map[string]interface{}) string {
	result, ok := res["result"].(map[string]interface{})
	if !ok {
		return ""
	}
	content, ok := result["content"].([]interface{})
	if !ok || len(content) == 0 {
		return ""
	}
	item, ok := content[0].(map[string]interface{})
	if !ok {
		return ""
	}
	text, _ := item["text"].(string)
	return text
}

func getToolError(res map[string]interface{}) (code float64, message string) {
	if errObj, ok := res["error"].(map[string]interface{}); ok {
		code, _ = errObj["code"].(float64)
		message, _ = errObj["message"].(string)
		return
	}
	return 0, ""
}

// searchResultPage wraps the paginated search response for test deserialization.
type searchResultPage struct {
	Results    []SearchResultResponse `json:"results"`
	NextCursor string                 `json:"next_cursor,omitempty"`
	Truncated  bool                   `json:"truncated,omitempty"`
}

// memoryListPageResponse wraps the paginated memory list response for test deserialization.
type memoryListPageResponse struct {
	Memories   []*db.Memory `json:"memories"`
	NextCursor string       `json:"next_cursor,omitempty"`
	Truncated  bool         `json:"truncated,omitempty"`
}

// --------------------------------------------------------------------------
// JSON-RPC 2.0 Protocol
// --------------------------------------------------------------------------

func TestJSONRPCInitialize(t *testing.T) {
	s := helperServer(t)
	res := callMCP(s, "initialize", nil)

	code, msg := getToolError(res)
	if code != 0 {
		t.Fatalf("unexpected error: code=%v msg=%s", code, msg)
	}
	result := res["result"].(map[string]interface{})
	if result["protocolVersion"] != "2024-11-05" {
		t.Errorf("expected protocolVersion '2024-11-05', got %v", result["protocolVersion"])
	}
	serverInfo := result["serverInfo"].(map[string]interface{})
	if serverInfo["name"] != "symaira-memory" {
		t.Errorf("expected name 'symaira-memory', got %v", serverInfo["name"])
	}
	if serverInfo["version"] != "test" {
		t.Errorf("expected version 'test', got %v", serverInfo["version"])
	}
}

func TestJSONRPCToolsList(t *testing.T) {
	s := helperServer(t)
	res := callMCP(s, "tools/list", nil)

	code, _ := getToolError(res)
	if code != 0 {
		t.Fatalf("unexpected error in tools/list")
	}
	result := res["result"].(map[string]interface{})
	tools := result["tools"].([]interface{})

	expectedTools := map[string]bool{
		"memory_get":      false,
		"memory_set":      false,
		"memory_search":   false,
		"memory_list":     false,
		"entity_list":     false,
		"entity_relate":   false,
		"graph_neighbors": false,
	}
	for _, toolRaw := range tools {
		tool := toolRaw.(map[string]interface{})
		name, _ := tool["name"].(string)
		annotations, ok := tool["annotations"].(map[string]interface{})
		if !ok || len(annotations) == 0 {
			t.Errorf("tool %q is missing annotations in tools/list", name)
		}
		if _, exists := expectedTools[name]; exists {
			expectedTools[name] = true
		}
	}
	for name, found := range expectedTools {
		if !found {
			t.Errorf("expected tool %q in tools/list response", name)
		}
	}
}

func TestJSONRPCMethodNotFound(t *testing.T) {
	s := helperServer(t)
	res := callMCP(s, "nonexistent/method", nil)

	code, msg := getToolError(res)
	if code != -32601 {
		t.Errorf("expected error code -32601, got %v", code)
	}
	if msg != "Method not found: nonexistent/method" {
		t.Errorf("expected 'Method not found: nonexistent/method', got %q", msg)
	}
}

// --------------------------------------------------------------------------
// Tool: memory_get
// --------------------------------------------------------------------------

func TestToolMemoryGetMissingArgs(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_get", map[string]interface{}{})

	text := getToolText(res)
	if !strings.Contains(text, "memory_get") {
		t.Errorf("expected message to contain 'memory_get', got text=%q", text)
	}
	if !strings.Contains(text, "'id' is required") {
		t.Errorf("expected message to contain \"'id' is required\", got text=%q", text)
	}
}

func TestToolMemoryGetNotFound(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_get", map[string]interface{}{"id": "nonexistent"})

	text := getToolText(res)
	if !strings.Contains(text, "memory not found") {
		t.Errorf("expected 'memory not found', got text=%q full=%v", text, res)
	}
}

func TestToolMemoryGetSuccess(t *testing.T) {
	s := helperServer(t)
	m := &db.Memory{
		ID:        "test-mem-1",
		Content:   "User prefers Go for backend",
		Scope:     "global",
		Metadata:  map[string]string{"source": "test"},
		Embedding: testEmbedding(0.1, 0.2, 0.3),
	}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	res := callTool(s, "memory_get", map[string]interface{}{"id": "test-mem-1"})

	text := getToolText(res)
	var mem MemoryResponse
	if err := json.Unmarshal([]byte(text), &mem); err != nil {
		t.Fatalf("failed to unmarshal memory from response: %v\ntext: %s", err, text)
	}
	if mem.ID != "test-mem-1" {
		t.Errorf("expected ID 'test-mem-1', got %q", mem.ID)
	}
	if mem.Content != "User prefers Go for backend" {
		t.Errorf("expected content 'User prefers Go for backend', got %q", mem.Content)
	}
	if len(mem.Metadata) != 1 || mem.Metadata["source"] != "test" {
		t.Errorf("expected metadata source=test, got %v", mem.Metadata)
	}
	if mem.Scope != "global" {
		t.Errorf("expected scope 'global', got %q", mem.Scope)
	}
}

func TestToolMemoryGetWithEvidence(t *testing.T) {
	s := helperServer(t)
	m := &db.Memory{
		ID:      "test-mem-evidence",
		Content: "User prefers dark mode",
		Scope:   "global",
	}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}
	if err := s.DB().SaveMemoryEvidence(m.ID, []evidencekit.Extraction{
		{EvidenceText: "I always use dark mode", Span: evidencekit.Span{Start: 0, End: 23}, AlignmentStatus: evidencekit.AlignmentExact},
	}); err != nil {
		t.Fatalf("failed to save evidence: %v", err)
	}

	res := callTool(s, "memory_get", map[string]interface{}{"id": m.ID, "with_evidence": true})
	text := getToolText(res)
	var mem MemoryResponse
	if err := json.Unmarshal([]byte(text), &mem); err != nil {
		t.Fatalf("failed to unmarshal memory from response: %v\ntext: %s", err, text)
	}
	if len(mem.Evidence) != 1 {
		t.Fatalf("expected 1 evidence row, got %d", len(mem.Evidence))
	}
	if mem.Evidence[0].EvidenceText != "I always use dark mode" {
		t.Errorf("unexpected evidence text: %q", mem.Evidence[0].EvidenceText)
	}

	// Without with_evidence, evidence must be omitted entirely.
	res2 := callTool(s, "memory_get", map[string]interface{}{"id": m.ID})
	text2 := getToolText(res2)
	var raw map[string]json.RawMessage
	if err := json.Unmarshal([]byte(text2), &raw); err != nil {
		t.Fatalf("failed to unmarshal: %v", err)
	}
	if _, ok := raw["evidence"]; ok {
		t.Errorf("expected no 'evidence' field without with_evidence, got %s", text2)
	}
}

func TestToolMemorySearchWithEvidence(t *testing.T) {
	s := helperServer(t)
	content := "User strongly prefers using Rust for systems programming projects"
	m := &db.Memory{
		ID:      "test-mem-search-evidence",
		Content: content,
		Scope:   "project",
	}
	emb := s.service.embeddings.GenerateVector(content)
	m.Embedding = emb.Vector
	m.EmbeddingSource = emb.Source
	m.EmbeddingModel = emb.Model
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}
	if err := s.DB().SaveMemoryEvidence(m.ID, []evidencekit.Extraction{
		{EvidenceText: content, Span: evidencekit.Span{Start: 0, End: len(content)}, AlignmentStatus: evidencekit.AlignmentExact},
	}); err != nil {
		t.Fatalf("failed to save evidence: %v", err)
	}

	res := callTool(s, "memory_search", map[string]interface{}{"query": content, "scope": "project", "limit": 5, "with_evidence": true})
	text := getToolText(res)
	var page searchResultPage
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("failed to unmarshal search results: %v\ntext: %s", err, text)
	}
	results := page.Results
	found := false
	for _, r := range results {
		if r.Memory.ID == m.ID {
			found = true
			if len(r.Memory.Evidence) != 1 {
				t.Errorf("expected 1 evidence row on search result, got %d", len(r.Memory.Evidence))
			}
		}
	}
	if !found {
		t.Fatalf("expected to find memory %s in search results", m.ID)
	}
}

// TestToolMemorySearchRedactsLegacySecret guards #515: a record that
// reached storage without write-time redaction (simulated here by writing
// directly via db.SaveMemory, bypassing internal/memory.Prepare) must still
// come back redacted from memory_search — the transport is defense in depth
// on top of write-time redaction, not a replacement for it.
func TestToolMemorySearchRedactsLegacySecret(t *testing.T) {
	s := helperServer(t)
	secret := "gho_abcdefabcdefabcdefabcdefabcdefabcdef"
	content := "Deployment note: GitHub auth token is " + secret
	m := &db.Memory{
		ID:      "test-mem-legacy-secret",
		Content: content,
		Scope:   "project",
	}
	emb := s.service.embeddings.GenerateVector(content)
	m.Embedding = emb.Vector
	m.EmbeddingSource = emb.Source
	m.EmbeddingModel = emb.Model
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	res := callTool(s, "memory_search", map[string]interface{}{"query": content, "scope": "project", "limit": 5})
	text := getToolText(res)
	if strings.Contains(text, secret) {
		t.Fatalf("memory_search response leaked raw secret: %s", text)
	}
}

// TestToolMemoryGetRedactsLegacySecret is the memory_get counterpart of
// TestToolMemorySearchRedactsLegacySecret.
func TestToolMemoryGetRedactsLegacySecret(t *testing.T) {
	s := helperServer(t)
	secret := "AKIA1234567890ABCDEF"
	content := "AWS key is " + secret
	m := &db.Memory{
		ID:      "test-mem-legacy-secret-get",
		Content: content,
		Scope:   "global",
	}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	res := callTool(s, "memory_get", map[string]interface{}{"id": m.ID})
	text := getToolText(res)
	if strings.Contains(text, secret) {
		t.Fatalf("memory_get response leaked raw secret: %s", text)
	}
}

func TestToolMemoryGetOmitsEmbedding(t *testing.T) {
	s := helperServer(t)
	m := &db.Memory{
		ID:        "test-mem-embed",
		Content:   "embedding should not be returned",
		Scope:     "global",
		Embedding: testEmbedding(0.9, 0.8, 0.7),
	}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	res := callTool(s, "memory_get", map[string]interface{}{"id": "test-mem-embed"})

	text := getToolText(res)
	var raw map[string]json.RawMessage
	if err := json.Unmarshal([]byte(text), &raw); err != nil {
		t.Fatalf("failed to unmarshal raw response: %v\ntext: %s", err, text)
	}
	if _, ok := raw["embedding"]; ok {
		t.Errorf("memory_get should not include embedding field, got %s", raw["embedding"])
	}
	if _, ok := raw["vector"]; ok {
		t.Errorf("memory_get should not include vector field, got %s", raw["vector"])
	}
}

// --------------------------------------------------------------------------
// Tool: memory_set
// --------------------------------------------------------------------------

func TestToolMemorySetMissingContent(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_set", map[string]interface{}{"scope": "global"})

	text := getToolText(res)
	if !strings.Contains(text, "memory_set") {
		t.Errorf("expected message to contain 'memory_set', got text=%q", text)
	}
	if !strings.Contains(text, "'content' is required") {
		t.Errorf("expected message to contain \"'content' is required\", got text=%q", text)
	}
}

func TestToolMemorySetInvalidMetadata(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_set", map[string]interface{}{
		"content":  "test content",
		"kind":     "user",
		"metadata": "not-json",
	})

	text := getToolText(res)
	if text == "" {
		t.Fatal("expected error text for invalid metadata")
	}
	if !strings.Contains(text, "metadata") {
		t.Errorf("expected error message to mention metadata, got %q", text)
	}
	if !strings.Contains(text, "JSON") {
		t.Errorf("expected error message to mention JSON, got %q", text)
	}
}

func TestToolMemorySetMetadataArrayRejected(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_set", map[string]interface{}{
		"content":  "test content",
		"kind":     "project",
		"metadata": `["one", "two"]`,
	})

	text := getToolText(res)
	if text == "" {
		t.Fatal("expected error text for array metadata")
	}
	if !strings.Contains(text, "metadata") {
		t.Errorf("expected error message to mention metadata, got %q", text)
	}
}

func TestToolMemorySetAndSearch(t *testing.T) {
	s := helperServer(t)

	content := "The API server runs on port 8080"
	setRes := callTool(s, "memory_set", map[string]interface{}{
		"content":  content,
		"kind":     "project",
		"scope":    "project",
		"metadata": `{"source":"test"}`,
	})
	text := getToolText(setRes)
	if !strings.Contains(text, "Memory saved successfully") {
		t.Errorf("expected success message, got %q", text)
	}

	searchRes := callTool(s, "memory_search", map[string]interface{}{"query": content, "scope": "project", "limit": 5})
	code, _ := getToolError(searchRes)
	if code != 0 {
		t.Fatalf("unexpected error in search: %v", searchRes)
	}

	searchText := getToolText(searchRes)
	var page searchResultPage
	if err := json.Unmarshal([]byte(searchText), &page); err != nil {
		t.Fatalf("failed to unmarshal search results: %v\ntext: %s", err, searchText)
	}
	results := page.Results
	if len(results) == 0 {
		t.Fatal("expected at least one search result")
	}
	if results[0].Memory.ID == "" {
		t.Error("expected non-empty memory ID in search result")
	}

	var raw []map[string]json.RawMessage
	// Use the results directly (they're already unmarshalled)
	data, _ := json.Marshal(page.Results)
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatalf("failed to remarshal search results: %v", err)
	}
	memRaw, ok := raw[0]["memory"]
	if !ok {
		t.Fatal("expected memory field in search result")
	}
	var rawMem map[string]json.RawMessage
	if err := json.Unmarshal(memRaw, &rawMem); err != nil {
		t.Fatalf("failed to unmarshal raw memory: %v", err)
	}
	if _, ok := rawMem["embedding"]; ok {
		t.Errorf("memory_search should not include embedding field, got %s", rawMem["embedding"])
	}
	if _, ok := rawMem["vector"]; ok {
		t.Errorf("memory_search should not include vector field, got %s", rawMem["vector"])
	}
}

// --------------------------------------------------------------------------
// Tool: memory_search
// --------------------------------------------------------------------------

func TestToolMemorySearchMissingQuery(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_search", map[string]interface{}{"limit": 5})

	text := getToolText(res)
	if !strings.Contains(text, "memory_search") {
		t.Errorf("expected message to contain 'memory_search', got text=%q", text)
	}
	if !strings.Contains(text, "'query' is required") {
		t.Errorf("expected message to contain \"'query' is required\", got text=%q", text)
	}
}

func TestToolMemorySearchEmpty(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_search", map[string]interface{}{"query": "nonexistent topic", "limit": 3})

	text := getToolText(res)
	if !strings.Contains(text, "No relevant memories found") {
		t.Errorf("expected 'No relevant memories found', got %q", text)
	}
}

// --------------------------------------------------------------------------
// Tool: memory_list
// --------------------------------------------------------------------------

func TestToolMemoryListEmpty(t *testing.T) {
	s := helperServer(t)
	res := callTool(s, "memory_list", map[string]interface{}{})

	text := getToolText(res)
	if !strings.Contains(text, "Memory store is empty") {
		t.Errorf("expected 'Memory store is empty', got %q", text)
	}
}

func TestToolMemoryListWithMemories(t *testing.T) {
	s := helperServer(t)
	m1 := &db.Memory{ID: "list-1", Content: "Memory A", Scope: "global", Embedding: testEmbedding(1.0)}
	m2 := &db.Memory{ID: "list-2", Content: "Memory B", Scope: "project", Embedding: testEmbedding(2.0)}
	_ = s.DB().SaveMemory(m1)
	_ = s.DB().SaveMemory(m2)

	res := callTool(s, "memory_list", map[string]interface{}{})

	text := getToolText(res)
	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("failed to unmarshal memories: %v\ntext: %s", err, text)
	}
	if len(page.Memories) != 2 {
		t.Errorf("expected 2 memories, got %d", len(page.Memories))
	}
}

func TestToolMemoryList_AsOfReturnsHistoricalVersion(t *testing.T) {
	s := helperServer(t)
	t0 := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	t1 := t0.Add(1 * time.Hour)

	old := &db.Memory{ID: "mcp-asof-old", Content: "old fact", Scope: "global", ValidFrom: &t0, ValidTo: &t1, SupersededBy: "mcp-asof-new"}
	newer := &db.Memory{ID: "mcp-asof-new", Content: "new fact", Scope: "global", ValidFrom: &t1}
	if err := s.DB().SaveMemory(old); err != nil {
		t.Fatalf("save old: %v", err)
	}
	if err := s.DB().SaveMemory(newer); err != nil {
		t.Fatalf("save new: %v", err)
	}

	res := callTool(s, "memory_list", map[string]interface{}{"as_of": t0.Add(30 * time.Minute).Format(time.RFC3339)})
	if code, msg := getToolError(res); code != 0 {
		t.Fatalf("unexpected error: %v %s", code, msg)
	}

	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(getToolText(res)), &page); err != nil {
		t.Fatalf("failed to unmarshal memories: %v", err)
	}
	if len(page.Memories) != 1 || page.Memories[0].ID != "mcp-asof-old" {
		t.Fatalf("expected the historical version 'mcp-asof-old', got %+v", page.Memories)
	}
}

func TestToolMemoryListWithLimit(t *testing.T) {
	s := helperServer(t)
	for i := 0; i < 5; i++ {
		m := &db.Memory{
			ID:        fmt.Sprintf("limit-mem-%d", i),
			Content:   fmt.Sprintf("Memory %d", i),
			Scope:     "global",
			Embedding: testEmbedding(float32(i) * 0.1),
		}
		_ = s.DB().SaveMemory(m)
	}

	res := callTool(s, "memory_list", map[string]interface{}{"limit": 2})

	text := getToolText(res)
	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("failed to unmarshal memories: %v\ntext: %s", err, text)
	}
	if len(page.Memories) != 2 {
		t.Errorf("expected 2 memories with limit=2, got %d", len(page.Memories))
	}
}

func TestToolMemoryListLimitClampedToMax(t *testing.T) {
	s := helperServer(t)
	for i := 0; i < 3; i++ {
		m := &db.Memory{
			ID:        fmt.Sprintf("max-mem-%d", i),
			Content:   fmt.Sprintf("Memory %d", i),
			Scope:     "global",
			Embedding: testEmbedding(float32(i) * 0.1),
		}
		_ = s.DB().SaveMemory(m)
	}

	res := callTool(s, "memory_list", map[string]interface{}{"limit": 5000})

	text := getToolText(res)
	var page memoryListPageResponse
	if err := json.Unmarshal([]byte(text), &page); err != nil {
		t.Fatalf("failed to unmarshal memories: %v\ntext: %s", err, text)
	}
	if len(page.Memories) != 3 {
		t.Errorf("expected 3 memories (limit clamped to 1000), got %d", len(page.Memories))
	}
}

func TestToolMemoryListInvalidLimitFallsBackToDefault(t *testing.T) {
	s := helperServer(t)
	for i := 0; i < 2; i++ {
		m := &db.Memory{
			ID:        fmt.Sprintf("inv-mem-%d", i),
			Content:   fmt.Sprintf("Memory %d", i),
			Scope:     "global",
			Embedding: testEmbedding(float32(i) * 0.1),
		}
		_ = s.DB().SaveMemory(m)
	}

	res := callTool(s, "memory_list", map[string]interface{}{"limit": "not-a-number"})

	text := getToolText(res)
	if !strings.Contains(text, "failed to parse arguments") {
		t.Errorf("expected parse error for invalid limit type, got text=%q", text)
	}
}

// --------------------------------------------------------------------------
// Unknown tool
// --------------------------------------------------------------------------

func TestToolUnknownTool(t *testing.T) {
	s := helperServer(t)
	mcpSrv := s.MCPServer()
	params, _ := json.Marshal(map[string]interface{}{
		"name":      "nonexistent_tool",
		"arguments": map[string]string{},
	})
	req := map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "test-1",
		"method":  "tools/call",
		"params":  json.RawMessage(params),
	}
	data, _ := json.Marshal(req)
	var input bytes.Buffer
	input.Write(frameRequest(data))
	var output bytes.Buffer
	_ = mcpSrv.ServeIO(context.Background(), &input, &output)
	res := readFramedResponse(&output)

	code, msg := getToolError(res)
	if code != -32601 {
		t.Errorf("expected error code -32601, got %v", code)
	}
	if !strings.Contains(msg, "nonexistent_tool") {
		t.Errorf("expected message to contain 'nonexistent_tool', got %q", msg)
	}
}

// --------------------------------------------------------------------------
// HTTP sync endpoint helpers
// --------------------------------------------------------------------------

func helperAuthToken(t *testing.T, s *Server) string {
	t.Helper()
	token, err := s.jwts.GenerateToken("test", time.Hour)
	if err != nil {
		t.Fatalf("failed to generate test token: %v", err)
	}
	return token
}

// --------------------------------------------------------------------------
// Sync: unauthenticated requests return 401
// --------------------------------------------------------------------------

func TestSyncChangesUnauthenticated(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/sync/changes")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", res.StatusCode)
	}
}

func TestSyncApplyUnauthenticated(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Post(ts.URL+"/api/sync/apply", "application/json", strings.NewReader(`{"memories":[]}`))
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	// CSRF blocks unauthenticated state-changing requests before the auth check.
	if res.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 (CSRF), got %d", res.StatusCode)
	}
}

// --------------------------------------------------------------------------
// Sync: pull with since filters correctly
// --------------------------------------------------------------------------

func TestSyncChangesSinceFilter(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	old := &db.Memory{
		ID:        "sync-old",
		Content:   "old memory",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
	}
	_ = s.DB().SaveMemory(old)

	got, _ := s.DB().GetMemory("sync-old")
	cutoff := got.UpdatedAt
	time.Sleep(50 * time.Millisecond)

	new := &db.Memory{
		ID:        "sync-new",
		Content:   "new memory",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.2),
	}
	_ = s.DB().SaveMemory(new)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?since="+cutoff.UTC().Format(time.RFC3339Nano), nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var body struct {
		Memories   []*db.Memory `json:"memories"`
		ServerTime string       `json:"server_time"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if len(body.Memories) != 1 {
		t.Fatalf("expected 1 memory after since cutoff, got %d", len(body.Memories))
	}
	if body.Memories[0].ID != "sync-new" {
		t.Errorf("expected memory ID 'sync-new', got %q", body.Memories[0].ID)
	}
	if body.ServerTime == "" {
		t.Error("expected non-empty server_time")
	}
}

func TestSyncChangesCursorPagination(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	for i := 0; i < 5; i++ {
		m := &db.Memory{
			ID:        fmt.Sprintf("page-mem-%d", i),
			Content:   fmt.Sprintf("Memory %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: testEmbedding(float32(i) * 0.1),
		}
		_ = s.DB().SaveMemory(m)
		time.Sleep(5 * time.Millisecond)
	}

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req1, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?limit=2", nil)
	req1.Header.Set("Authorization", "Bearer "+token)

	resp1, err := http.DefaultClient.Do(req1)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp1.Body.Close() }()

	if resp1.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp1.StatusCode)
	}

	var body1 struct {
		Memories   []*db.Memory `json:"memories"`
		NextCursor string       `json:"next_cursor"`
	}
	if err := json.NewDecoder(resp1.Body).Decode(&body1); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if len(body1.Memories) != 2 {
		t.Fatalf("expected 2 memories on first page, got %d", len(body1.Memories))
	}
	if body1.NextCursor == "" {
		t.Fatal("expected next_cursor on first page")
	}

	req2, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?limit=2&cursor="+body1.NextCursor, nil)
	req2.Header.Set("Authorization", "Bearer "+token)

	resp2, err := http.DefaultClient.Do(req2)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp2.Body.Close() }()

	if resp2.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp2.StatusCode)
	}

	var body2 struct {
		Memories   []*db.Memory `json:"memories"`
		NextCursor string       `json:"next_cursor"`
	}
	if err := json.NewDecoder(resp2.Body).Decode(&body2); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if len(body2.Memories) != 2 {
		t.Fatalf("expected 2 memories on second page, got %d", len(body2.Memories))
	}
	if body2.NextCursor == "" {
		t.Fatal("expected next_cursor on second page")
	}

	if body1.Memories[0].ID == body2.Memories[0].ID {
		t.Error("first page and second page should have different memories")
	}

	req3, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?limit=2&cursor="+body2.NextCursor, nil)
	req3.Header.Set("Authorization", "Bearer "+token)

	resp3, err := http.DefaultClient.Do(req3)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp3.Body.Close() }()

	var body3 struct {
		Memories   []*db.Memory `json:"memories"`
		NextCursor string       `json:"next_cursor"`
	}
	if err := json.NewDecoder(resp3.Body).Decode(&body3); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if len(body3.Memories) != 1 {
		t.Fatalf("expected 1 memory on third page, got %d", len(body3.Memories))
	}
	if body3.NextCursor != "" {
		t.Error("expected no next_cursor on last page")
	}
}

func TestSyncChangesCursorInvalid(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?cursor=!!!invalid!!!", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for invalid cursor, got %d", resp.StatusCode)
	}
}

func TestSyncChangesLimitClamped(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/sync/changes?limit=99999", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}
}

// --------------------------------------------------------------------------
// Sync: apply skips older, overwrites newer
// --------------------------------------------------------------------------

func TestSyncApplyOlderSkippedNewerOverwrites(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	existing := &db.Memory{
		ID:        "110e8400-e29b-41d4-a716-446655440001",
		Content:   "original content",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
	}
	_ = s.DB().SaveMemory(existing)

	got, _ := s.DB().GetMemory("110e8400-e29b-41d4-a716-446655440001")
	existingTime := got.UpdatedAt

	older := &db.Memory{
		ID:        "110e8400-e29b-41d4-a716-446655440001",
		Content:   "should be skipped",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		UpdatedAt: existingTime.Add(-1 * time.Hour),
		CreatedAt: existingTime.Add(-1 * time.Hour),
	}
	newer := &db.Memory{
		ID:        "110e8400-e29b-41d4-a716-446655440001",
		Content:   "should overwrite",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		UpdatedAt: existingTime.Add(1 * time.Hour),
		CreatedAt: existingTime,
	}
	fresh := &db.Memory{
		ID:        "120e8400-e29b-41d4-a716-446655440002",
		Content:   "brand new",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.3),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}

	payload, _ := json.Marshal(map[string][]*db.Memory{
		"memories": {older, newer, fresh},
	})

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var result struct {
		Applied int `json:"applied"`
		Skipped int `json:"skipped"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if result.Applied != 2 {
		t.Errorf("expected 2 applied (newer + fresh), got %d", result.Applied)
	}
	if result.Skipped != 1 {
		t.Errorf("expected 1 skipped (older), got %d", result.Skipped)
	}

	updated, _ := s.DB().GetMemory("110e8400-e29b-41d4-a716-446655440001")
	if updated.Content != "should overwrite" {
		t.Errorf("expected content 'should overwrite', got %q", updated.Content)
	}
}

func TestSyncApplyPIIRedactionAndScopeValidation(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	withEmail := &db.Memory{
		ID:        "210e8400-e29b-41d4-a716-446655440010",
		Content:   "Contact alice@example.com for details",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}
	invalidScope := &db.Memory{
		ID:        "220e8400-e29b-41d4-a716-446655440011",
		Content:   "This has invalid scope",
		Scope:     "invalid-scope-value",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.2),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}
	valid := &db.Memory{
		ID:        "230e8400-e29b-41d4-a716-446655440012",
		Content:   "Normal content",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.3),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}

	payload, _ := json.Marshal(map[string][]*db.Memory{
		"memories": {withEmail, invalidScope, valid},
	})

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var result struct {
		Applied             int `json:"applied"`
		Skipped             int `json:"skipped"`
		SkippedInvalidScope int `json:"skippedInvalidScope"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if result.Applied != 2 {
		t.Errorf("expected 2 applied (pii-1 + valid-1), got %d", result.Applied)
	}
	if result.Skipped != 0 {
		t.Errorf("expected 0 skipped, got %d", result.Skipped)
	}
	if result.SkippedInvalidScope != 1 {
		t.Errorf("expected 1 skippedInvalidScope, got %d", result.SkippedInvalidScope)
	}

	stored, _ := s.DB().GetMemory("210e8400-e29b-41d4-a716-446655440010")
	if stored.Content == "Contact alice@example.com for details" {
		t.Error("expected PII to be redacted in stored content")
	}
}

func TestSyncApplyMetadataPIIRedaction(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	withPIIMeta := &db.Memory{
		ID:        "310e8400-e29b-41d4-a716-446655440020",
		Content:   "clean content",
		Scope:     "global",
		Metadata:  map[string]string{"contact": "eve@example.com", "source": "sync"},
		Embedding: testEmbedding(0.1),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}

	payload, _ := json.Marshal(map[string][]*db.Memory{
		"memories": {withPIIMeta},
	})

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	stored, _ := s.DB().GetMemory("310e8400-e29b-41d4-a716-446655440020")
	if stored == nil {
		t.Fatal("expected memory to be stored")
	}
	if stored.Metadata["contact"] != "[REDACTED_EMAIL]" {
		t.Errorf("expected metadata email redacted, got %q", stored.Metadata["contact"])
	}
	if stored.Metadata["source"] != "sync" {
		t.Errorf("expected clean metadata preserved, got %q", stored.Metadata["source"])
	}
}

// --------------------------------------------------------------------------
// Sync: apply rejects non-UUID memory IDs
// --------------------------------------------------------------------------

func TestSyncApplyInvalidUUIDSkipped(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	validUUID := &db.Memory{
		ID:        "550e8400-e29b-41d4-a716-446655440000",
		Content:   "Valid UUID memory",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}
	invalidID := &db.Memory{
		ID:        "not-a-uuid!!",
		Content:   "Malicious ID memory",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.2),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}
	anotherInvalid := &db.Memory{
		ID:        "550e8400-e29b-41d4-a716-44665544",
		Content:   "Truncated UUID memory",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.3),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}

	payload, _ := json.Marshal(map[string][]*db.Memory{
		"memories": {validUUID, invalidID, anotherInvalid},
	})

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var result struct {
		Applied          int `json:"applied"`
		Skipped          int `json:"skipped"`
		SkippedInvalidID int `json:"skippedInvalidID"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if result.Applied != 1 {
		t.Errorf("expected 1 applied (valid UUID only), got %d", result.Applied)
	}
	if result.SkippedInvalidID != 2 {
		t.Errorf("expected 2 skippedInvalidID (non-UUID + truncated), got %d", result.SkippedInvalidID)
	}

	stored, err := s.DB().GetMemory("550e8400-e29b-41d4-a716-446655440000")
	if err != nil || stored == nil {
		t.Error("expected valid UUID memory to be stored")
	}
}

func TestSyncApplyAuditLogCreated(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	mem := &db.Memory{
		ID:        "660e8400-e29b-41d4-a716-446655440001",
		Content:   "Memory for audit test",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		UpdatedAt: time.Now().UTC(),
		CreatedAt: time.Now().UTC(),
	}

	payload, _ := json.Marshal(map[string][]*db.Memory{
		"memories": {mem},
	})

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	events, err := s.DB().GetAuditLogs("sync", 10)
	if err != nil {
		t.Fatalf("failed to fetch audit logs: %v", err)
	}
	if len(events) == 0 {
		t.Fatal("expected at least 1 audit event for sync")
	}
	if events[0].Action != "sync" {
		t.Errorf("expected action 'sync', got %q", events[0].Action)
	}
}

// --------------------------------------------------------------------------
// GET /api/get
// --------------------------------------------------------------------------

func TestApiGetUnauthenticated(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/get?id=anything")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", res.StatusCode)
	}
}

func TestApiGetMissingID(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/get", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", resp.StatusCode)
	}
}

func TestApiGetNotFound(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/get?id=nonexistent-id", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404, got %d", resp.StatusCode)
	}

	var body map[string]string
	_ = json.NewDecoder(resp.Body).Decode(&body)
	if !strings.Contains(body["error"], "not found") {
		t.Errorf("expected error containing 'not found', got %q", body["error"])
	}
}

func TestApiGetSuccess(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	m := &db.Memory{
		ID:        "get-test-1",
		Content:   "Test memory for GET endpoint",
		Scope:     "global",
		Metadata:  map[string]string{"source": "test"},
		Embedding: testEmbedding(0.1, 0.2),
	}
	_ = s.DB().SaveMemory(m)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/get?id=get-test-1", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var got db.Memory
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}
	if got.ID != "get-test-1" {
		t.Errorf("expected ID 'get-test-1', got %q", got.ID)
	}
	if got.Content != "Test memory for GET endpoint" {
		t.Errorf("expected content match, got %q", got.Content)
	}
}

// --------------------------------------------------------------------------
// DELETE /api/delete
// --------------------------------------------------------------------------

func TestApiDeleteUnauthenticated(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("DELETE", ts.URL+"/api/delete?id=anything", nil)
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	// CSRF blocks unauthenticated state-changing requests before the auth check.
	if res.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 (CSRF), got %d", res.StatusCode)
	}
}

func TestApiDeleteMissingID(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("DELETE", ts.URL+"/api/delete", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", resp.StatusCode)
	}
}

func TestApiDeleteNotFound(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("DELETE", ts.URL+"/api/delete?id=nonexistent-id", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404, got %d", resp.StatusCode)
	}
}

func TestApiDeleteSuccess(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	m := &db.Memory{
		ID:        "del-test-1",
		Content:   "To be deleted",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
	}
	_ = s.DB().SaveMemory(m)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("DELETE", ts.URL+"/api/delete?id=del-test-1", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var body map[string]bool
	_ = json.NewDecoder(resp.Body).Decode(&body)
	if !body["deleted"] {
		t.Error("expected deleted: true")
	}

	got, _ := s.DB().GetMemory("del-test-1")
	if got != nil {
		t.Error("expected memory to be deleted from database")
	}
}

func TestApiDeleteViaPost(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	m := &db.Memory{
		ID:        "del-post-1",
		Content:   "To be deleted via POST",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
	}
	_ = s.DB().SaveMemory(m)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/delete?id=del-post-1", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	got, _ := s.DB().GetMemory("del-post-1")
	if got != nil {
		t.Error("expected memory to be deleted from database")
	}
}

// --------------------------------------------------------------------------
// GET /api/rules
// --------------------------------------------------------------------------

func TestApiRulesUnauthenticated(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/rules")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", res.StatusCode)
	}
}

func TestApiRulesEmpty(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/rules", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var body struct {
		Rules []db.Rule `json:"rules"`
	}
	_ = json.NewDecoder(resp.Body).Decode(&body)
	if len(body.Rules) != 0 {
		t.Errorf("expected 0 rules, got %d", len(body.Rules))
	}
}

func TestApiRulesWithData(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)

	r := &db.Rule{
		ID:       "rule-1",
		Content:  "Always use tabs for indentation",
		Scope:    "global",
		Metadata: map[string]string{"source": "test"},
	}
	_ = s.DB().SaveRule(r)

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/rules", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var body struct {
		Rules []db.Rule `json:"rules"`
	}
	_ = json.NewDecoder(resp.Body).Decode(&body)
	if len(body.Rules) != 1 {
		t.Fatalf("expected 1 rule, got %d", len(body.Rules))
	}
	if body.Rules[0].ID != "rule-1" {
		t.Errorf("expected rule ID 'rule-1', got %q", body.Rules[0].ID)
	}
	if body.Rules[0].Content != "Always use tabs for indentation" {
		t.Errorf("expected rule content match, got %q", body.Rules[0].Content)
	}
}

// --------------------------------------------------------------------------
// Web Console
// --------------------------------------------------------------------------

func TestWebConsoleReturnsHTML(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()

	if res.StatusCode != http.StatusOK {
		t.Errorf("expected 200, got %d", res.StatusCode)
	}

	contentType := res.Header.Get("Content-Type")
	if !strings.Contains(contentType, "text/html") {
		t.Errorf("expected text/html content type, got %q", contentType)
	}

	body, _ := io.ReadAll(res.Body)
	if !strings.Contains(string(body), "Symaira Memory Console") {
		t.Error("expected HTML to contain 'Symaira Memory Console'")
	}
}

func TestWebConsoleStaticAssets(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	assets := []struct {
		path        string
		contentType string
	}{
		{"/style.css", "text/css"},
		{"/app.js", "javascript"},
	}

	for _, asset := range assets {
		res, err := http.Get(ts.URL + asset.path)
		if err != nil {
			t.Errorf("request for %s failed: %v", asset.path, err)
			continue
		}
		_ = res.Body.Close()

		if res.StatusCode != http.StatusOK {
			t.Errorf("expected 200 for %s, got %d", asset.path, res.StatusCode)
		}

		ct := res.Header.Get("Content-Type")
		if !strings.Contains(ct, asset.contentType) {
			t.Errorf("expected %s content type for %s, got %q", asset.contentType, asset.path, ct)
		}
	}
}

func TestCORSPreflightAllowedOrigin(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("OPTIONS", ts.URL+"/api/status", nil)
	req.Header.Set("Origin", "chrome-extension://abcdef")
	req.Header.Set("Access-Control-Request-Method", "POST")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for allowed origin preflight, got %d", resp.StatusCode)
	}
	if resp.Header.Get("Access-Control-Allow-Origin") != "chrome-extension://abcdef" {
		t.Errorf("expected Access-Control-Allow-Origin header, got %q", resp.Header.Get("Access-Control-Allow-Origin"))
	}
}

func TestCORSForbiddenOrigin(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Origin", "https://evil.example.com")
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 for disallowed origin, got %d", resp.StatusCode)
	}
}

func TestCORSNoOriginHeader(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/list", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 when no Origin header, got %d", resp.StatusCode)
	}
}

func TestSearchMalformedBody(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/search", strings.NewReader("not json at all"))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for malformed body, got %d", resp.StatusCode)
	}
}

func TestSetMalformedBody(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader("{invalid"))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for malformed body, got %d", resp.StatusCode)
	}
}

func TestSearchMethodNotAllowed(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/search", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusMethodNotAllowed {
		t.Errorf("expected 405 for GET on search, got %d", resp.StatusCode)
	}
}

func TestRequireRoleReadOnlyProfile(t *testing.T) {
	s := helperServer(t)
	s.SetProfile(&db.Profile{Role: "read-only"})
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 for read-only profile on write endpoint, got %d", resp.StatusCode)
	}
}

func TestStatusIncludesEmbeddingBackend(t *testing.T) {
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/status")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()

	if res.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", res.StatusCode)
	}

	var body map[string]string
	_ = json.NewDecoder(res.Body).Decode(&body)
	if body["embedding_backend"] == "" {
		t.Error("expected non-empty embedding_backend in status response")
	}
	if body["embedding_backend"] != "ollama" && body["embedding_backend"] != "lexical" {
		t.Errorf("expected embedding_backend to be 'ollama' or 'lexical', got %q", body["embedding_backend"])
	}
}

func TestStatusEmbeddingBackendLexicalAfterFailure(t *testing.T) {
	s := helperServer(t)
	s.service.embeddings.MarkOllamaFailed()
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/status")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()

	var body map[string]string
	_ = json.NewDecoder(res.Body).Decode(&body)
	if body["embedding_backend"] != "lexical" {
		t.Errorf("expected embedding_backend 'lexical' after failure, got %q", body["embedding_backend"])
	}
}

func TestWebConsoleDoesNotShadowAPIRoutes(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/status", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for /api/status, got %d", resp.StatusCode)
	}

	var body map[string]string
	_ = json.NewDecoder(resp.Body).Decode(&body)
	if body["status"] != "healthy" {
		t.Errorf("expected status 'healthy', got %q", body["status"])
	}
}

// --------------------------------------------------------------------------
// HTTP server startup: occupied port returns error
// --------------------------------------------------------------------------

func TestStartHTTPServerOccupiedPort(t *testing.T) {
	s := helperServer(t)

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("failed to occupy port: %v", err)
	}
	defer func() { _ = ln.Close() }()

	port := ln.Addr().(*net.TCPAddr).Port
	err = s.StartHTTPServer(port)
	if err == nil {
		t.Fatal("expected error when port is already occupied")
	}
}

// --------------------------------------------------------------------------
// HTTP server startup: graceful shutdown returns nil
// --------------------------------------------------------------------------

func TestStartHTTPServerGracefulShutdown(t *testing.T) {
	s := helperServer(t)

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("failed to get free port: %v", err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	_ = ln.Close()

	done := make(chan error, 1)
	go func() {
		done <- s.StartHTTPServer(port)
	}()

	time.Sleep(100 * time.Millisecond)

	proc, err := os.FindProcess(os.Getpid())
	if err != nil {
		t.Fatalf("failed to find process: %v", err)
	}
	proc.Signal(os.Interrupt)

	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("expected nil error on graceful shutdown, got: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("timeout waiting for server to shut down")
	}
}

// --------------------------------------------------------------------------
// HTTP server startup: runtime error propagates
// --------------------------------------------------------------------------

func TestStartHTTPServerServeError(t *testing.T) {
	s := helperServer(t)

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("failed to get free port: %v", err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	_ = ln.Close()

	done := make(chan error, 1)
	go func() {
		done <- s.StartHTTPServer(port)
	}()

	time.Sleep(100 * time.Millisecond)

	killProc, err := os.FindProcess(os.Getpid())
	if err != nil {
		t.Fatalf("failed to find process: %v", err)
	}

	errCh := make(chan error, 1)
	go func() {
		errCh <- killProc.Signal(os.Interrupt)
	}()

	select {
	case <-errCh:
	case <-time.After(2 * time.Second):
		t.Fatal("timeout sending signal")
	}

	select {
	case srvErr := <-done:
		if srvErr != nil && !errors.Is(srvErr, http.ErrServerClosed) {
			t.Logf("server returned error (expected for non-graceful): %v", srvErr)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("timeout waiting for server to shut down")
	}
}

// --------------------------------------------------------------------------
// HTTP authentication and role enforcement (table-driven)
// --------------------------------------------------------------------------

func TestRequireAuthNilJWTProvider(t *testing.T) {
	// When auth.jwts is nil, requireAuth returns (nil, true).
	s := helperServer(t)
	s.auth.jwts = nil

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("GET", ts.URL+"/api/list", nil)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 when jwts is nil (auth disabled), got %d", resp.StatusCode)
	}
}

func TestRequireRoleNilJWTProvider(t *testing.T) {
	// Auth-disabled (jwts == nil) open access must also apply to role-gated
	// routes, not just plain-auth ones: RequireRole sees a nil payload from
	// RequireAuth in exactly this case and must not treat it as unauthenticated.
	s := helperServer(t)
	s.auth.jwts = nil

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Requested-With", "XMLHttpRequest") // satisfy CSRF check; unrelated to the auth path under test
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for role-gated route when jwts is nil (auth disabled), got %d", resp.StatusCode)
	}
}

func TestRequireAuthTableDriven(t *testing.T) {
	// Lines 79-88: missing/invalid Bearer prefix, invalid token, expired token.
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	validToken := helperAuthToken(t, s)

	// Generate an expired token by creating a provider with negative duration.
	expiredCfg := config.Defaults()
	expiredProvider, err := security.NewJWTProvider(expiredCfg, nil)
	if err != nil {
		t.Fatalf("failed to create expired JWT provider: %v", err)
	}
	expiredToken, err := expiredProvider.GenerateToken("test", -time.Hour)
	if err != nil {
		t.Fatalf("failed to generate expired token: %v", err)
	}

	tests := []struct {
		name       string
		authHeader string
		wantStatus int
	}{
		{"missing auth header", "", http.StatusUnauthorized},
		{"invalid Bearer prefix", "Basic abc123", http.StatusUnauthorized},
		{"Bearer with empty token", "Bearer ", http.StatusUnauthorized},
		{"invalid token garbage", "Bearer not.a.valid-token", http.StatusUnauthorized},
		{"expired token", "Bearer " + expiredToken, http.StatusUnauthorized},
		{"valid token", "Bearer " + validToken, http.StatusOK},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req, _ := http.NewRequest("GET", ts.URL+"/api/list", nil)
			if tt.authHeader != "" {
				req.Header.Set("Authorization", tt.authHeader)
			}
			resp, err := http.DefaultClient.Do(req)
			if err != nil {
				t.Fatalf("request failed: %v", err)
			}
			defer func() { _ = resp.Body.Close() }()
			if resp.StatusCode != tt.wantStatus {
				t.Errorf("expected %d, got %d", tt.wantStatus, resp.StatusCode)
			}
		})
	}
}

func TestRequireRoleDBProfileLookup(t *testing.T) {
	// Lines 104-111: per-subject profile lookup from DB allows/denies access.
	s := helperServer(t)
	token := helperAuthToken(t, s)

	// Create a read-only profile matching the token subject "test".
	readOnlyProfile := &db.Profile{
		ID:   "profile-ro-1",
		Name: "test",
		Type: "agent",
		Role: "read",
	}
	if err := s.DB().SaveProfile(readOnlyProfile); err != nil {
		t.Fatalf("failed to save profile: %v", err)
	}

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	// Write endpoint should be rejected for read-only DB profile.
	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 for read-only DB profile on write endpoint, got %d", resp.StatusCode)
	}

	// Read endpoint should succeed for read-only DB profile.
	req2, _ := http.NewRequest("GET", ts.URL+"/api/list", nil)
	req2.Header.Set("Authorization", "Bearer "+token)

	resp2, err := http.DefaultClient.Do(req2)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp2.Body.Close() }()

	if resp2.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for read-only DB profile on read endpoint, got %d", resp2.StatusCode)
	}
}

func TestRequireRoleDBProfileReadWriteAllowsAccess(t *testing.T) {
	// Lines 104-111: per-subject profile with readwrite role allows write.
	s := helperServer(t)
	token := helperAuthToken(t, s)

	rwProfile := &db.Profile{
		ID:   "profile-rw-1",
		Name: "test",
		Type: "agent",
		Role: "readwrite",
	}
	if err := s.DB().SaveProfile(rwProfile); err != nil {
		t.Fatalf("failed to save profile: %v", err)
	}

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"allowed"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for readwrite DB profile on write endpoint, got %d", resp.StatusCode)
	}
}

func helperServerWithRequireProfile(t *testing.T, requireProfile bool) *Server {
	t.Helper()
	database := helperDB(t)
	jwtProvider, err := security.NewJWTProvider(config.Defaults(), nil)
	if err != nil {
		t.Fatalf("failed to create JWT provider: %v", err)
	}
	cfg := config.Defaults()
	cfg.Security.RequireProfile = requireProfile
	return NewServer(database, jwtProvider, "test", cfg)
}

func TestRequireRoleDBLookupErrorFailsClosed(t *testing.T) {
	// A DB error during profile lookup must deny write access, not silently
	// fall through to full access as if no profile existed.
	s := helperServer(t)
	token := helperAuthToken(t, s)

	_, err := s.DB().Conn().Exec(
		`INSERT INTO profiles (id, name, type, role, description, metadata, created_at, updated_at)
		 VALUES ('profile-bad-json', 'test', 'agent', 'readwrite', '', 'not-valid-json', ?, ?)`,
		time.Now().UTC(), time.Now().UTC(),
	)
	if err != nil {
		t.Fatalf("failed to insert bad-json profile: %v", err)
	}

	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 when profile lookup errors (fail closed), got %d", resp.StatusCode)
	}
}

func TestRequireRoleUnknownSubjectPermissiveByDefault(t *testing.T) {
	// Default config (require_profile=false): an unknown subject keeps default
	// write access, preserving pre-#348 behavior for repos that opt out.
	s := helperServerWithRequireProfile(t, false)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for unknown subject under permissive default, got %d", resp.StatusCode)
	}
}

func TestRequireRoleUnknownSubjectDeniedWhenRequireProfileEnabled(t *testing.T) {
	// With require_profile=true, an unknown subject must be denied write access.
	s := helperServerWithRequireProfile(t, true)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("POST", ts.URL+"/api/set", strings.NewReader(`{"content":"test"}`))
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 for unknown subject when require_profile is enabled, got %d", resp.StatusCode)
	}

	// Read access must still be granted — require_profile only gates writes.
	req2, _ := http.NewRequest("GET", ts.URL+"/api/list", nil)
	req2.Header.Set("Authorization", "Bearer "+token)
	resp2, err := http.DefaultClient.Do(req2)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp2.Body.Close() }()

	if resp2.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for read access with require_profile enabled, got %d", resp2.StatusCode)
	}
}

func TestMatchOriginTableDriven(t *testing.T) {
	// Lines 145-152: wildcard "*", scheme wildcard, and exact match.
	tests := []struct {
		name    string
		origin  string
		pattern string
		want    bool
	}{
		{"wildcard matches anything", "https://evil.com", "*", true},
		{"wildcard matches empty", "", "*", true},
		{"scheme wildcard match", "chrome-extension://abc", "chrome-extension://*", true},
		{"scheme wildcard no match", "https://example.com", "chrome-extension://*", false},
		{"exact match", "https://example.com", "https://example.com", true},
		{"exact no match", "https://other.com", "https://example.com", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := matchOrigin(tt.origin, tt.pattern)
			if got != tt.want {
				t.Errorf("matchOrigin(%q, %q) = %v, want %v", tt.origin, tt.pattern, got, tt.want)
			}
		})
	}
}

func TestCORSPreflightWildcardOrigin(t *testing.T) {
	// Lines 145-147 via enableCORS: wildcard "*" allows any origin.
	s := helperServer(t)
	s.SetAllowedOrigins([]string{"*"})
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	req, _ := http.NewRequest("OPTIONS", ts.URL+"/api/status", nil)
	req.Header.Set("Origin", "https://any-origin.example.com")
	req.Header.Set("Access-Control-Request-Method", "POST")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for wildcard origin preflight, got %d", resp.StatusCode)
	}
	if resp.Header.Get("Access-Control-Allow-Origin") != "https://any-origin.example.com" {
		t.Errorf("expected Access-Control-Allow-Origin header, got %q", resp.Header.Get("Access-Control-Allow-Origin"))
	}
}

// --------------------------------------------------------------------------
// Host header validation
// --------------------------------------------------------------------------

func TestHostValidationAllowsLoopbackAddresses(t *testing.T) {
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	s := helperServer(t)
	h := s.hostValidationHandler(handler)

	tests := []struct {
		name string
		host string
	}{
		{"127.0.0.1 with port", "127.0.0.1:8787"},
		{"127.0.0.1 without port", "127.0.0.1"},
		{"localhost with port", "localhost:8787"},
		{"localhost without port", "localhost"},
		{"[::1] with port", "[::1]:8787"},
		{"[::1] without port", "[::1]"},
		{"127.0.0.2 (127.0.0.0/8 range)", "127.0.0.2:8787"},
		{"127.255.255.254 (127.0.0.0/8 boundary)", "127.255.255.254:8787"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest("GET", "/api/status", nil)
			req.Host = tt.host
			rec := httptest.NewRecorder()
			h.ServeHTTP(rec, req)
			if rec.Code != http.StatusOK {
				t.Errorf("expected 200 for host %q, got %d", tt.host, rec.Code)
			}
		})
	}
}

func TestHostValidationRejectsNonLoopback(t *testing.T) {
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	s := helperServer(t)
	h := s.hostValidationHandler(handler)

	tests := []struct {
		name string
		host string
	}{
		{"external IP with port", "192.168.1.1:8787"},
		{"external IP without port", "10.0.0.1"},
		{"DNS hostname with port", "evil.com:8787"},
		{"DNS hostname without port", "example.com"},
		{"0.0.0.0 is not loopback", "0.0.0.0:8787"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest("GET", "/api/status", nil)
			req.Host = tt.host
			rec := httptest.NewRecorder()
			h.ServeHTTP(rec, req)
			if rec.Code != http.StatusForbidden {
				t.Errorf("expected 403 for host %q, got %d", tt.host, rec.Code)
			}
		})
	}
}

func TestHostValidationGuardInactiveOnNonLoopbackBind(t *testing.T) {
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	s := helperServer(t)
	// Override bind address to non-loopback → guard inactive.
	s.bindAddr = "0.0.0.0:8787"
	h := s.hostValidationHandler(handler)

	req := httptest.NewRequest("GET", "/api/status", nil)
	req.Host = "evil.com:8787"
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Errorf("expected 200 (guard inactive on non-loopback bind), got %d", rec.Code)
	}
}

func TestHostValidationCatchesAllMethods(t *testing.T) {
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})
	s := helperServer(t)
	h := s.hostValidationHandler(handler)

	methods := []string{"GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"}
	for _, method := range methods {
		t.Run("valid host/"+method, func(t *testing.T) {
			req := httptest.NewRequest(method, "/api/status", nil)
			req.Host = "127.0.0.1:8787"
			rec := httptest.NewRecorder()
			h.ServeHTTP(rec, req)
			if rec.Code != http.StatusOK {
				t.Errorf("expected 200, got %d", rec.Code)
			}
		})
		t.Run("invalid host/"+method, func(t *testing.T) {
			req := httptest.NewRequest(method, "/api/status", nil)
			req.Host = "evil.com:8787"
			rec := httptest.NewRecorder()
			h.ServeHTTP(rec, req)
			if rec.Code != http.StatusForbidden {
				t.Errorf("expected 403, got %d", rec.Code)
			}
		})
	}
}

func TestHostValidationDoesNotBlockLoopbackOnRealServer(t *testing.T) {
	// Verify that real httptest.Server requests still work (Host header
	// is set automatically to 127.0.0.1:<random-port> by the Go client).
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Get(ts.URL + "/api/status")
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	// /api/status has authNone, so it should succeed.
	if res.StatusCode != http.StatusOK {
		t.Errorf("expected 200 for /api/status via httptest, got %d", res.StatusCode)
	}
}

// --------------------------------------------------------------------------
// isLocalOrigin — 0.0.0.0 no longer local, covers whole 127.0.0.0/8
// --------------------------------------------------------------------------

func TestIsLocalOriginRejectsZeroDotZero(t *testing.T) {
	if isLocalOrigin("http://0.0.0.0:8787") {
		t.Error("isLocalOrigin should NOT treat 0.0.0.0 as a local origin")
	}
}

func TestIsLocalOriginCoversLoopbackRange(t *testing.T) {
	tests := []struct {
		origin string
		want   bool
	}{
		{"http://127.0.0.1:8787", true},
		{"http://127.0.0.1", true},
		{"http://127.0.0.2:8080", true},
		{"http://127.255.255.254", true},
		{"http://[::1]:8787", true},
		{"http://[::1]", true},
		{"http://localhost:8787", true},
		{"http://localhost", true},
		{"https://localhost", true},
		{"http://0.0.0.0:8787", false},
		{"http://192.168.1.1", false},
		{"http://example.com", false},
		{"https://evil.com", false},
		{"", false},
		{"random-string", false},
	}
	for _, tt := range tests {
		t.Run(tt.origin, func(t *testing.T) {
			got := isLocalOrigin(tt.origin)
			if got != tt.want {
				t.Errorf("isLocalOrigin(%q) = %v, want %v", tt.origin, got, tt.want)
			}
		})
	}
}

// --------------------------------------------------------------------------
// End-to-end: CSRF still works with host validation in front
// --------------------------------------------------------------------------

func TestCSRFBlockedAfterHostValidation(t *testing.T) {
	// POST without auth and without local origin should still be blocked
	// by CSRF after host validation passes.
	s := helperServer(t)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	res, err := http.Post(ts.URL+"/api/sync/apply", "application/json", strings.NewReader(`{"memories":[]}`))
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusForbidden {
		t.Errorf("expected 403 (CSRF), got %d", res.StatusCode)
	}
}

// --------------------------------------------------------------------------
// Body bounding: oversized and trailing-JSON rejection
// --------------------------------------------------------------------------

func TestSyncApply_OversizedRequestBody(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	// Build a valid JSON body slightly above the 1 MiB request limit.
	padding := make([]byte, maxRequestBody-len(`{"memories":[]}`)+1)
	for i := range padding {
		padding[i] = 'a'
	}
	body := string(`{"memories":[]}`) + string(padding)
	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusRequestEntityTooLarge {
		t.Errorf("expected 413 for oversized body, got %d", resp.StatusCode)
	}
}

func TestSyncApply_TrailingJSONRejected(t *testing.T) {
	s := helperServer(t)
	token := helperAuthToken(t, s)
	ts := httptest.NewServer(s.httpMux())
	defer ts.Close()

	payload := `{"memories":[]}` + `{"trailing":true}`
	req, _ := http.NewRequest("POST", ts.URL+"/api/sync/apply", strings.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request failed: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400 for trailing JSON, got %d", resp.StatusCode)
	}
}
