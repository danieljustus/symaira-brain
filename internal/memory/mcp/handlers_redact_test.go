package mcp

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// TestHandleSearchRedactsLegacySecret guards #515 on the HTTP API transport:
// a record written without write-time redaction (simulated via a direct
// db.SaveMemory call, bypassing internal/memory.Prepare) must still come
// back redacted from POST /api/search.
func TestHandleSearchRedactsLegacySecret(t *testing.T) {
	s := helperServer(t)
	secret := "gho_abcdefabcdefabcdefabcdefabcdefabcdef"
	content := "Deployment note: GitHub auth token is " + secret
	m := &db.Memory{ID: "http-legacy-secret-search", Content: content, Scope: "project"}
	emb := s.service.embeddings.GenerateVector(content)
	m.Embedding = emb.Vector
	m.EmbeddingSource = emb.Source
	m.EmbeddingModel = emb.Model
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	body, _ := json.Marshal(map[string]interface{}{"query": content, "scope": "project", "limit": 5})
	req := httptest.NewRequest(http.MethodPost, "/api/search", bytes.NewReader(body))
	w := httptest.NewRecorder()
	s.handleSearch(w, req)

	if got := w.Body.String(); strings.Contains(got, secret) {
		t.Fatalf("POST /api/search response leaked raw secret: %s", got)
	}
}

// TestHandleListRedactsLegacySecret is the GET /api/list counterpart.
func TestHandleListRedactsLegacySecret(t *testing.T) {
	s := helperServer(t)
	secret := "AKIA1234567890ABCDEF"
	content := "AWS key is " + secret
	m := &db.Memory{ID: "http-legacy-secret-list", Content: content, Scope: "global"}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	req := httptest.NewRequest(http.MethodGet, "/api/list?scope=global", nil)
	w := httptest.NewRecorder()
	s.handleList(w, req)

	if got := w.Body.String(); strings.Contains(got, secret) {
		t.Fatalf("GET /api/list response leaked raw secret: %s", got)
	}
}

// TestHandleGetRedactsLegacySecret is the GET /api/get counterpart.
func TestHandleGetRedactsLegacySecret(t *testing.T) {
	s := helperServer(t)
	secret := "sk-abcdefghijklmnopqrstuvwxyz"
	content := "API key " + secret
	m := &db.Memory{ID: "http-legacy-secret-get", Content: content, Scope: "global"}
	if err := s.DB().SaveMemory(m); err != nil {
		t.Fatalf("failed to save test memory: %v", err)
	}

	req := httptest.NewRequest(http.MethodGet, "/api/get?id="+m.ID, nil)
	w := httptest.NewRecorder()
	s.handleGet(w, req)

	if got := w.Body.String(); strings.Contains(got, secret) {
		t.Fatalf("GET /api/get response leaked raw secret: %s", got)
	}
}
