package mcp

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// helperServerWithReceipts builds a test server with the recall-receipt
// flag forced to the given value.
func helperServerWithReceipts(t *testing.T, receipts bool) *Server {
	t.Helper()
	database := helperDB(t)
	cfg := config.Defaults()
	cfg.MCP.RecallReceipts = receipts
	jwtProvider, err := security.NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create JWT provider: %v", err)
	}
	return NewServer(database, jwtProvider, "test", cfg)
}

// saveSearchableMemory persists a memory with a real embedding so a search
// can return it.
func saveSearchableMemory(t *testing.T, s *Server, id, content string) {
	t.Helper()
	m := &db.Memory{ID: id, Content: content, Scope: "project"}
	emb := s.service.embeddings.GenerateVector(content)
	m.Embedding = emb.Vector
	m.EmbeddingSource = emb.Source
	m.EmbeddingModel = emb.Model
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}
}

func TestToolMemorySearchMintsRecallReceipt(t *testing.T) {
	s := helperServerWithReceipts(t, true)
	content := "The daemon listens on port 8787."
	saveSearchableMemory(t, s, "test-mem-receipt-on", content)

	res := callTool(s, "memory_search", map[string]interface{}{"query": content, "scope": "project", "limit": 5})
	var page searchResultPage
	if err := json.Unmarshal([]byte(getToolText(res)), &page); err != nil {
		t.Fatalf("failed to unmarshal search results: %v", err)
	}

	found := false
	for _, r := range page.Results {
		if r.Memory.ID == "test-mem-receipt-on" {
			found = true
			if !strings.HasPrefix(r.Receipt, "◉ memory: ") {
				t.Errorf("expected a minted receipt, got %q", r.Receipt)
			}
			if !strings.Contains(r.Receipt, "(project,") {
				t.Errorf("receipt missing scope: %q", r.Receipt)
			}
		}
	}
	if !found {
		t.Fatalf("expected memory in search results")
	}
}

func TestToolMemorySearchOmitsReceiptWhenDisabled(t *testing.T) {
	s := helperServerWithReceipts(t, false)
	content := "No receipt should appear."
	saveSearchableMemory(t, s, "test-mem-receipt-off", content)

	res := callTool(s, "memory_search", map[string]interface{}{"query": content, "scope": "project", "limit": 5})
	var page searchResultPage
	if err := json.Unmarshal([]byte(getToolText(res)), &page); err != nil {
		t.Fatalf("failed to unmarshal search results: %v", err)
	}

	for _, r := range page.Results {
		if r.Memory.ID == "test-mem-receipt-off" {
			if r.Receipt != "" {
				t.Errorf("receipt must be omitted when disabled, got %q", r.Receipt)
			}
			return
		}
	}
	t.Fatalf("expected memory in search results")
}
