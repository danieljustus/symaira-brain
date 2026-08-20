package llm

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestNewClientDefaults(t *testing.T) {
	client := NewClient("", "", 0)

	if client.OllamaURL != "http://localhost:11434/api/generate" {
		t.Errorf("expected default OllamaURL, got %s", client.OllamaURL)
	}
	if client.OllamaModel != "llama3" {
		t.Errorf("expected default OllamaModel 'llama3', got %s", client.OllamaModel)
	}
	if client.HTTPClient == nil {
		t.Error("expected HTTPClient to be set")
	}
	if client.HTTPClient.Timeout != 45*time.Second {
		t.Errorf("expected 45s timeout, got %v", client.HTTPClient.Timeout)
	}
}

func TestNewClientCustomParams(t *testing.T) {
	client := NewClient("http://custom:11434/api/generate", "mistral", 0)

	if client.OllamaURL != "http://custom:11434/api/generate" {
		t.Errorf("expected custom OllamaURL, got %s", client.OllamaURL)
	}
	if client.OllamaModel != "mistral" {
		t.Errorf("expected custom OllamaModel 'mistral', got %s", client.OllamaModel)
	}
}

func TestQueryOllamaSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.Header.Get("Content-Type") != "application/json" {
			t.Errorf("expected Content-Type application/json, got %s", r.Header.Get("Content-Type"))
		}

		var reqBody map[string]interface{}
		if err := json.NewDecoder(r.Body).Decode(&reqBody); err != nil {
			t.Fatalf("failed to decode request body: %v", err)
		}

		if reqBody["model"] != "llama3" {
			t.Errorf("expected model 'llama3', got %v", reqBody["model"])
		}
		if reqBody["system"] != "system" {
			t.Errorf("expected system 'system', got %v", reqBody["system"])
		}
		// Verify the format field is a JSON schema object, not the string "json".
		fmtVal, ok := reqBody["format"]
		if !ok {
			t.Fatal("expected format field in request body")
		}
		fmtMap, ok := fmtVal.(map[string]interface{})
		if !ok {
			t.Fatalf("expected format to be a JSON object (schema), got %T", fmtVal)
		}
		if fmtMap["type"] != "object" {
			t.Errorf("expected schema type 'object', got %v", fmtMap["type"])
		}

		w.Header().Set("Content-Type", "application/x-ndjson")
		fmt.Fprintln(w, `{"model":"llama3","response":"test response","done":true}`)
	}))
	defer server.Close()

	client := NewClient(server.URL, "llama3", 0)
	result, err := client.QueryOllama(context.Background(), "system", "user")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "test response" {
		t.Errorf("expected 'test response', got %s", result)
	}
}

// TestQueryOllamaAccumulatesChunks verifies that a multi-chunk streamed
// response is concatenated into the final result, not just the last chunk.
func TestQueryOllamaAccumulatesChunks(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/x-ndjson")
		fmt.Fprintln(w, `{"model":"llama3","response":"hello ","done":false}`)
		fmt.Fprintln(w, `{"model":"llama3","response":"world","done":true}`)
	}))
	defer server.Close()

	client := NewClient(server.URL, "llama3", 0)
	result, err := client.QueryOllama(context.Background(), "system", "user")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "hello world" {
		t.Errorf("expected 'hello world', got %q", result)
	}
}

func TestQueryOllamaHTTPError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	client := NewClient(server.URL, "llama3", 0)
	_, err := client.QueryOllama(context.Background(), "system", "user")
	if err == nil {
		t.Fatal("expected error for HTTP 500")
	}
}

func TestQueryOllamaMalformedJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/x-ndjson")
		_, _ = w.Write([]byte("not json\n"))
	}))
	defer server.Close()

	client := NewClient(server.URL, "llama3", 0)
	_, err := client.QueryOllama(context.Background(), "system", "user")
	if err == nil {
		t.Fatal("expected error for malformed JSON")
	}
}

func TestQueryOllamaContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t.Fatal("should not reach handler")
	}))
	defer server.Close()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	client := NewClient(server.URL, "llama3", 0)
	_, err := client.QueryOllama(ctx, "system", "user")
	if err == nil {
		t.Fatal("expected error for cancelled context")
	}
}

func TestQueryOpenAISuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.Header.Get("Authorization") != "Bearer test-key" {
			t.Errorf("expected Bearer test-key, got %s", r.Header.Get("Authorization"))
		}

		var reqBody map[string]interface{}
		if err := json.NewDecoder(r.Body).Decode(&reqBody); err != nil {
			t.Fatalf("failed to decode request body: %v", err)
		}

		if reqBody["model"] != "gpt-4o-mini" {
			t.Errorf("expected model 'gpt-4o-mini', got %v", reqBody["model"])
		}

		// Verify response_format uses json_schema with the schema object.
		rfVal, ok := reqBody["response_format"]
		if !ok {
			t.Fatal("expected response_format field")
		}
		rfMap, ok := rfVal.(map[string]interface{})
		if !ok {
			t.Fatalf("expected response_format to be an object, got %T", rfVal)
		}
		if rfMap["type"] != "json_schema" {
			t.Errorf("expected response_format type 'json_schema', got %v", rfMap["type"])
		}
		jsVal, ok := rfMap["json_schema"]
		if !ok {
			t.Fatal("expected json_schema field inside response_format")
		}
		jsMap, ok := jsVal.(map[string]interface{})
		if !ok {
			t.Fatalf("expected json_schema to be an object, got %T", jsVal)
		}
		if jsMap["name"] != "consolidation_result" {
			t.Errorf("expected json_schema name 'consolidation_result', got %v", jsMap["name"])
		}
		if jsMap["strict"] != true {
			t.Errorf("expected json_schema strict true, got %v", jsMap["strict"])
		}
		schemaVal, ok := jsMap["schema"]
		if !ok {
			t.Fatal("expected schema field inside json_schema")
		}
		schemaMap, ok := schemaVal.(map[string]interface{})
		if !ok {
			t.Fatalf("expected schema to be an object, got %T", schemaVal)
		}
		if schemaMap["type"] != "object" {
			t.Errorf("expected schema type 'object', got %v", schemaMap["type"])
		}

		messages := reqBody["messages"].([]interface{})
		if len(messages) != 2 {
			t.Fatalf("expected 2 messages, got %d", len(messages))
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]interface{}{
			"choices": []map[string]interface{}{
				{
					"message": map[string]string{
						"content": "openai response",
					},
				},
			},
		})
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	result, err := client.QueryOpenAI(context.Background(), "system", "user", "test-key", "", server.URL)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "openai response" {
		t.Errorf("expected 'openai response', got %s", result)
	}
}

func TestQueryOpenAICustomModel(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var reqBody map[string]interface{}
		_ = json.NewDecoder(r.Body).Decode(&reqBody)

		if reqBody["model"] != "gpt-4" {
			t.Errorf("expected model 'gpt-4', got %v", reqBody["model"])
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]interface{}{
			"choices": []map[string]interface{}{
				{
					"message": map[string]string{
						"content": "response",
					},
				},
			},
		})
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	result, err := client.QueryOpenAI(context.Background(), "system", "user", "test-key", "gpt-4", server.URL)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "response" {
		t.Errorf("expected 'response', got %s", result)
	}
}

func TestQueryOpenAIHTTPError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	_, err := client.QueryOpenAI(context.Background(), "system", "user", "bad-key", "", server.URL)
	if err == nil {
		t.Fatal("expected error for HTTP 401")
	}
}

func TestQueryOpenAIEmptyChoices(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]interface{}{
			"choices": []interface{}{},
		})
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	_, err := client.QueryOpenAI(context.Background(), "system", "user", "test-key", "", server.URL)
	if err == nil {
		t.Fatal("expected error for empty choices")
	}
}

func TestQueryOpenAIMalformedJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte("not json"))
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	_, err := client.QueryOpenAI(context.Background(), "system", "user", "test-key", "", server.URL)
	if err == nil {
		t.Fatal("expected error for malformed JSON")
	}
}

func TestQueryOllamaProvider(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/x-ndjson")
		fmt.Fprintln(w, `{"model":"llama3","response":"ollama result","done":true}`)
	}))
	defer server.Close()

	client := NewClient(server.URL, "llama3", 0)
	result, err := client.Query(context.Background(), "system", "user", "ollama", "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "ollama result" {
		t.Errorf("expected 'ollama result', got %s", result)
	}
}

func TestConsolidationResponseSchema(t *testing.T) {
	schema := ConsolidationResponseSchema()

	// Verify top-level structure
	if schema["type"] != "object" {
		t.Errorf("expected schema type 'object', got %v", schema["type"])
	}

	props, ok := schema["properties"].(map[string]any)
	if !ok {
		t.Fatal("expected properties to be a map")
	}

	// Verify consolidated field
	cons, ok := props["consolidated"].(map[string]any)
	if !ok {
		t.Fatal("expected consolidated to be a map")
	}
	if cons["type"] != "array" {
		t.Errorf("expected consolidated type 'array', got %v", cons["type"])
	}

	// Verify discarded_ids field
	disc, ok := props["discarded_ids"].(map[string]any)
	if !ok {
		t.Fatal("expected discarded_ids to be a map")
	}
	if disc["type"] != "array" {
		t.Errorf("expected discarded_ids type 'array', got %v", disc["type"])
	}

	// Verify required fields
	required, ok := schema["required"].([]any)
	if !ok {
		t.Fatal("expected required to be a slice")
	}
	if len(required) != 2 {
		t.Errorf("expected 2 required fields, got %d", len(required))
	}

	// Verify schema serializes to valid JSON
	data, err := json.Marshal(schema)
	if err != nil {
		t.Errorf("schema should serialize to valid JSON: %v", err)
	}

	// Verify the serialized schema is a JSON object (not a string)
	var parsed any
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Errorf("schema JSON should be valid: %v", err)
	}
	if _, ok := parsed.(map[string]any); !ok {
		t.Error("expected schema JSON to be an object")
	}
}

func TestQueryOpenAIProvider(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]interface{}{
			"choices": []map[string]interface{}{
				{
					"message": map[string]string{
						"content": "openai result",
					},
				},
			},
		})
	}))
	defer server.Close()

	client := NewClient("", "", 0)
	result, err := client.QueryOpenAI(context.Background(), "system", "user", "test-key", "", server.URL)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "openai result" {
		t.Errorf("expected 'openai result', got %s", result)
	}
}

func TestQueryOpenAIProviderNoKey(t *testing.T) {
	t.Setenv("OPENAI_API_KEY", "")

	client := NewClient("", "", 0)
	_, err := client.Query(context.Background(), "system", "user", "openai", "")
	if err == nil {
		t.Fatal("expected error for missing API key")
	}
}

func TestQueryOpenAIProviderUsesEnvKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer env-key" {
			t.Errorf("expected Bearer env-key, got %s", r.Header.Get("Authorization"))
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]interface{}{
			"choices": []map[string]interface{}{
				{
					"message": map[string]string{
						"content": "result",
					},
				},
			},
		})
	}))
	defer server.Close()

	t.Setenv("OPENAI_API_KEY", "env-key")

	client := NewClient("", "", 0)
	result, err := client.QueryOpenAI(context.Background(), "system", "user", "env-key", "", server.URL)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result != "result" {
		t.Errorf("expected 'result', got %s", result)
	}
}
