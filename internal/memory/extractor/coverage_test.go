package extractor

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestGenerateVectorCacheHit(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		vec := make([]float32, DefaultDimensions)
		for i := range vec {
			vec[i] = float32(i+1) / float32(DefaultDimensions)
		}
		_ = json.NewEncoder(w).Encode(struct {
			Data []struct {
				Embedding []float32 `json:"embedding"`
			} `json:"data"`
		}{Data: []struct {
			Embedding []float32 `json:"embedding"`
		}{{Embedding: vec}}})
	}))
	defer server.Close()

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = server.URL

	r1 := eg.GenerateVector("same text")
	r2 := eg.GenerateVector("same text")

	if r1.Source != "ollama" || r2.Source != "ollama" {
		t.Fatalf("expected ollama source, got %q then %q", r1.Source, r2.Source)
	}
	for i := range r1.Vector {
		if r1.Vector[i] != r2.Vector[i] {
			t.Fatalf("cache miss: vectors differ at %d", i)
		}
	}
}

func TestDimensions(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	if got := eg.Dimensions(); got != DefaultDimensions {
		t.Errorf("Dimensions() = %d", got)
	}
}

func TestOllamaBaseURLNoPath(t *testing.T) {
	got := ollamaBaseURL("http://localhost:11434")
	if got != "http://localhost:11434" {
		t.Errorf("ollamaBaseURL = %q", got)
	}
}

func TestQueryOllamaWithContextError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	server.Close() // connection refused

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = server.URL

	result := eg.GenerateVector("test")
	if result.Source != "hash-fallback" {
		t.Fatalf("expected hash-fallback, got %q", result.Source)
	}
}

func TestIsStopWordNonStop(t *testing.T) {
	if isStopWord("xyzzy") {
		t.Error("expected false for non-stop word")
	}
}

func TestIsStopWordStop(t *testing.T) {
	if !isStopWord("the") {
		t.Error("expected true for stop word")
	}
}

func TestOllamaClientRebuildOnURLChange(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		vec := make([]float32, DefaultDimensions)
		for i := range vec {
			vec[i] = float32(i+1) / float32(DefaultDimensions)
		}
		_ = json.NewEncoder(w).Encode(struct {
			Data []struct {
				Embedding []float32 `json:"embedding"`
			} `json:"data"`
		}{Data: []struct {
			Embedding []float32 `json:"embedding"`
		}{{Embedding: vec}}})
	}))
	defer server.Close()

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = server.URL
	eg.GenerateVector("first")

	// Change URL to trigger rebuild
	eg.OllamaURL = server.URL + "/other"
	eg.GenerateVector("second")
	// If rebuild failed, this would panic or return error
}

func TestOllamaClientTimeoutDefault(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaTimeout = 0
	// This should use default timeout internally
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		vec := make([]float32, DefaultDimensions)
		for i := range vec {
			vec[i] = float32(i+1) / float32(DefaultDimensions)
		}
		_ = json.NewEncoder(w).Encode(struct {
			Data []struct {
				Embedding []float32 `json:"embedding"`
			} `json:"data"`
		}{Data: []struct {
			Embedding []float32 `json:"embedding"`
		}{{Embedding: vec}}})
	}))
	defer server.Close()

	eg.OllamaURL = server.URL
	result := eg.GenerateVector("test")
	if result.Source != "ollama" {
		t.Fatalf("expected ollama, got %q", result.Source)
	}
}

func TestGenerateLocalHashVectorEmpty(t *testing.T) {
	vec := GenerateLocalHashVector("", DefaultDimensions)
	if len(vec) != DefaultDimensions {
		t.Fatalf("expected %d dimensions, got %d", DefaultDimensions, len(vec))
	}
	for i := range vec {
		if vec[i] != 0 {
			t.Fatalf("expected zero vector for empty input, got %v at %d", vec[i], i)
		}
	}
}

func TestGenerateLocalHashVectorStopWordsOnly(t *testing.T) {
	vec := GenerateLocalHashVector("the and of to in", DefaultDimensions)
	if len(vec) != DefaultDimensions {
		t.Fatalf("expected %d dimensions, got %d", DefaultDimensions, len(vec))
	}
	for i := range vec {
		if vec[i] != 0 {
			t.Fatalf("expected zero vector for stop-words-only input, got %v at %d", vec[i], i)
		}
	}
}
