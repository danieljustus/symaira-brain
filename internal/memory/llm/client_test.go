package llm

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestNewClientDefaults(t *testing.T) {
	t.Setenv("OPENAI_API_KEY", "")
	c := NewClient("", "", "", 0)

	if c.OllamaBase != "http://localhost:11434" {
		t.Errorf("expected default OllamaBase, got %s", c.OllamaBase)
	}
	if c.OllamaModel != "llama3" {
		t.Errorf("expected default OllamaModel 'llama3', got %s", c.OllamaModel)
	}
	if c.Provider != "ollama" {
		t.Errorf("without OPENAI_API_KEY the provider must default to ollama, got %q", c.Provider)
	}
	if c.resolveTimeout() != 45*time.Second {
		t.Errorf("expected 45s timeout, got %v", c.resolveTimeout())
	}
}

func TestNewClientAutoDetectsOpenAI(t *testing.T) {
	t.Setenv("OPENAI_API_KEY", "test-key")
	c := NewClient("", "", "", 0)
	if c.Provider != "openai" {
		t.Errorf("provider autodetect = %q, want openai", c.Provider)
	}
}

func TestNewClientCustomParams(t *testing.T) {
	c := NewClient("http://custom:11434", "mistral", "ollama", 7*time.Second)
	if c.OllamaBase != "http://custom:11434" || c.OllamaModel != "mistral" {
		t.Errorf("custom params not applied: %+v", c)
	}
	if c.resolveTimeout() != 7*time.Second {
		t.Errorf("timeout = %v", c.resolveTimeout())
	}
}

// ollamaCaptureServer records the native generate request and streams the
// given NDJSON chunks.
func ollamaCaptureServer(t *testing.T, chunks ...string) (*httptest.Server, func() map[string]any) {
	t.Helper()
	var got map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&got); err != nil {
			t.Errorf("decode request: %v", err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		w.Header().Set("Content-Type", "application/x-ndjson")
		for _, c := range chunks {
			_, _ = fmt.Fprintln(w, c)
		}
	}))
	return server, func() map[string]any { return got }
}

func TestQueryOllamaWire(t *testing.T) {
	server, got := ollamaCaptureServer(t, `{"model":"llama3","response":"hello ","done":false}`, `{"model":"llama3","response":"world","done":true}`)
	defer server.Close()

	c := NewClient(server.URL, "llama3", "ollama", 0)
	out, err := c.QueryWithSchema(context.Background(), "be strict", "user task", "ollama", ConsolidationResponseSchema())
	if err != nil {
		t.Fatalf("QueryWithSchema: %v", err)
	}
	if out != "hello world" {
		t.Errorf("accumulated stream = %q", out)
	}
	req := got()
	if req["model"] != "llama3" || req["system"] != "be strict" {
		t.Errorf("request = %v", req)
	}
	if req["stream"] != true {
		t.Errorf("stream flag = %v", req["stream"])
	}
	format, ok := req["format"].(map[string]any)
	if !ok {
		t.Fatalf("format field must be a JSON schema object, got %T", req["format"])
	}
	if format["type"] != "object" {
		t.Errorf("schema type = %v", format["type"])
	}
}

func TestQueryOllamaHTTPError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	c := NewClient(server.URL, "llama3", "ollama", 0)
	if _, err := c.Query(context.Background(), "s", "u", "ollama"); err == nil {
		t.Fatal("expected error for HTTP 500")
	}
}

func TestQueryOpenAIWire(t *testing.T) {
	var got struct {
		Model          string              `json:"model"`
		Messages       []map[string]string `json:"messages"`
		ResponseFormat any                 `json:"response_format"`
	}
	var gotAuth string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		if err := json.NewDecoder(r.Body).Decode(&got); err != nil {
			t.Errorf("decode: %v", err)
		}
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"{\"consolidated\":[]}"},"finish_reason":"stop"}]}`)
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "test-key-123")
	c := NewClient(server.URL, "llama3", "openai", 0)
	c.LLMURL = server.URL
	out, err := c.Query(context.Background(), "sys", "user", "openai")
	if err != nil {
		t.Fatalf("Query: %v", err)
	}
	if !strings.Contains(out, "consolidated") {
		t.Errorf("content = %q", out)
	}
	if gotAuth != "Bearer test-key-123" {
		t.Errorf("auth header = %q", gotAuth)
	}
	if got.ResponseFormat == nil {
		t.Error("response_format must be pinned on the openai path")
	}
	// The system prompt is merged as the first message.
	if len(got.Messages) == 0 || got.Messages[0]["role"] != "system" || got.Messages[0]["content"] != "sys" {
		t.Errorf("messages = %v, want system prompt first", got.Messages)
	}
}

func TestQueryOpenAIEmptyChoices(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"choices":[]}`))
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "k")
	c := NewClient(server.URL, "", "openai", 0)
	c.LLMURL = server.URL
	if _, err := c.Query(context.Background(), "s", "u", "openai"); err == nil {
		t.Fatal("expected empty-choices error")
	}
}

func TestConsolidationResponseSchema(t *testing.T) {
	schema := ConsolidationResponseSchema()
	raw, err := json.Marshal(schema)
	if err != nil {
		t.Fatalf("schema must be marshalable: %v", err)
	}
	var parsed struct {
		Type       string `json:"type"`
		Required   []any  `json:"required"`
		Properties struct {
			Consolidated struct {
				Type  string `json:"type"`
				Items struct {
					Required []any `json:"required"`
				} `json:"items"`
			} `json:"consolidated"`
			Discarded struct {
				Type string `json:"type"`
			} `json:"discarded_ids"`
		} `json:"properties"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatalf("schema must be valid JSON: %v", err)
	}
	if parsed.Type != "object" {
		t.Errorf("schema type = %q", parsed.Type)
	}
	if len(parsed.Required) != 2 {
		t.Errorf("top-level required = %v", parsed.Required)
	}
	if parsed.Properties.Consolidated.Type != "array" {
		t.Errorf("consolidated = %+v", parsed.Properties.Consolidated)
	}
	if len(parsed.Properties.Consolidated.Items.Required) != 3 {
		t.Errorf("item required = %v", parsed.Properties.Consolidated.Items.Required)
	}
}
