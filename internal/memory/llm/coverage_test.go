package llm

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestOpenAIClientErrorWhenNoAPIKey(t *testing.T) {
	t.Setenv("OPENAI_API_KEY", "")
	c := NewClient("", "", "openai", 0)
	_, err := c.openaiClient()
	if err == nil {
		t.Fatal("expected openaiClient error when OPENAI_API_KEY is unset")
	}
}

func TestQueryWithSchemaEmptyProviderFallsBack(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"ok"}}]}`)
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "k")
	c := NewClient(server.URL, "", "openai", 0)
	c.LLMURL = server.URL
	out, err := c.QueryWithSchema(context.Background(), "s", "u", "", ConsolidationResponseSchema())
	if err != nil {
		t.Fatalf("QueryWithSchema: %v", err)
	}
	if out != "ok" {
		t.Errorf("content = %q", out)
	}
}

func TestQueryOllamaGenerateError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	c := NewClient(server.URL, "llama3", "ollama", 0)
	if _, err := c.Query(context.Background(), "s", "u", "ollama"); err == nil {
		t.Fatal("expected error from Generate")
	}
}

func TestQueryOpenAIChatError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "k")
	c := NewClient(server.URL, "", "openai", 0)
	c.LLMURL = server.URL
	if _, err := c.Query(context.Background(), "s", "u", "openai"); err == nil {
		t.Fatal("expected error from Chat")
	}
}

func TestOllamaRootNoPath(t *testing.T) {
	got := ollamaRoot("http://localhost:11434")
	if got != "http://localhost:11434" {
		t.Errorf("ollamaRoot = %q", got)
	}
}

func TestOllamaRootWithPath(t *testing.T) {
	got := ollamaRoot("http://localhost:11434/v1")
	if got != "http://localhost:11434" {
		t.Errorf("ollamaRoot = %q", got)
	}
}

func TestQueryOpenAIClientError(t *testing.T) {
	t.Setenv("OPENAI_API_KEY", "")
	c := NewClient("", "", "openai", 0)
	_, err := c.queryOpenAI(context.Background(), "s", "u", ConsolidationResponseSchema())
	if err == nil {
		t.Fatal("expected queryOpenAI error when openaiClient fails")
	}
}

type badSchemaValue struct{}

func (b badSchemaValue) MarshalJSON() ([]byte, error) {
	return nil, fmt.Errorf("intentional marshal error for test")
}

func TestQueryOpenAIMarshalError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"ok"}}]}`)
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "k")
	c := NewClient(server.URL, "", "openai", 0)
	c.LLMURL = server.URL
	schema := map[string]any{
		"type": "object",
		"bad":  badSchemaValue{},
	}
	_, err := c.QueryWithSchema(context.Background(), "s", "u", "openai", schema)
	if err == nil {
		t.Fatal("expected error from json.Marshal")
	}
	if !strings.Contains(err.Error(), "failed to encode response schema") {
		t.Errorf("unexpected error: %v", err)
	}
}
