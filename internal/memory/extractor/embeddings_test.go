package extractor

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestLocalHashVectorizer(t *testing.T) {
	dims := 768

	text1 := "User prefers typescript"
	text2 := "User prefers typescript" // identical
	text3 := "Different context statement"

	vec1 := GenerateLocalHashVector(text1, dims)
	vec2 := GenerateLocalHashVector(text2, dims)
	vec3 := GenerateLocalHashVector(text3, dims)

	// Test dimensions
	if len(vec1) != dims {
		t.Errorf("expected vector size %d, got %d", dims, len(vec1))
	}

	// Test determinism
	for i := 0; i < dims; i++ {
		if vec1[i] != vec2[i] {
			t.Errorf("vectorizer is not deterministic: mismatch at dimension %d", i)
		}
	}

	// Test L2 Normalization (sum of squares must be 1.0)
	var sumSquares float64
	for _, val := range vec1 {
		sumSquares += float64(val * val)
	}

	// Margin check (float precision delta)
	if sumSquares > 0.0 && mathAbs(float32(sumSquares)-1.0) > 1e-5 {
		t.Errorf("vector is not L2 normalized: sum of squares is %f", sumSquares)
	}

	// Test distinct contexts have different vector representations
	matches := 0
	for i := 0; i < dims; i++ {
		if vec1[i] == vec3[i] && vec1[i] != 0 {
			matches++
		}
	}
	if matches == dims {
		t.Errorf("distinct statements produced identical vectors")
	}
}

func TestActiveBackendDefaultOllama(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	if got := eg.ActiveBackend(); got != "ollama" {
		t.Errorf("expected ActiveBackend() to return 'ollama' on fresh instance, got %q", got)
	}
}

func TestActiveBackendLexicalAfterFailure(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	eg.mu.Lock()
	eg.lastFail = time.Now()
	eg.mu.Unlock()
	if got := eg.ActiveBackend(); got != "lexical" {
		t.Errorf("expected ActiveBackend() to return 'lexical' after recent failure, got %q", got)
	}
}

func TestActiveBackendRecoveryAfterCooldown(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	eg.mu.Lock()
	eg.lastFail = time.Now().Add(-ollamaCacheTTL - time.Second)
	eg.mu.Unlock()
	if got := eg.ActiveBackend(); got != "ollama" {
		t.Errorf("expected ActiveBackend() to return 'ollama' after cooldown, got %q", got)
	}
}

func TestGenerateVectorTimeoutFallsBack(t *testing.T) {
	slowServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(5 * time.Second)
		w.WriteHeader(http.StatusOK)
	}))
	defer slowServer.Close()

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = slowServer.URL
	eg.OllamaTimeout = 50 * time.Millisecond

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	start := time.Now()
	result := eg.GenerateVectorWithContext(ctx, "test query")
	elapsed := time.Since(start)

	if result.Source != "hash-fallback" {
		t.Errorf("expected hash-fallback on timeout, got %q", result.Source)
	}
	if elapsed > 500*time.Millisecond {
		t.Errorf("timeout fallback took too long: %v", elapsed)
	}
}

func TestGenerateVectorContextCanceledFallsBack(t *testing.T) {
	slowServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(5 * time.Second)
		w.WriteHeader(http.StatusOK)
	}))
	defer slowServer.Close()

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = slowServer.URL
	eg.OllamaTimeout = 5 * time.Second

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	result := eg.GenerateVectorWithContext(ctx, "test query")

	if result.Source != "hash-fallback" {
		t.Errorf("expected hash-fallback on canceled context, got %q", result.Source)
	}
}

func TestMetricsTracking(t *testing.T) {
	eg := NewEmbeddingsGenerator(nil)
	eg.MarkOllamaFailed()

	for i := 0; i < 5; i++ {
		eg.GenerateVector("test query")
	}

	metrics := eg.Metrics()
	if metrics.TotalRequests != 5 {
		t.Errorf("expected 5 total requests, got %d", metrics.TotalRequests)
	}
	if metrics.FallbackCount != 5 {
		t.Errorf("expected 5 fallbacks, got %d", metrics.FallbackCount)
	}
	if metrics.FallbackRate != 1.0 {
		t.Errorf("expected fallback rate 1.0, got %f", metrics.FallbackRate)
	}
}

func mathAbs(f float32) float32 {
	if f < 0 {
		return -f
	}
	return f
}

// TestGenerateVectorUsesOllamakitEmbedEndpoint guards the shared-transport
// switch (#438): the generator must talk to Ollama through ollamakit's
// /api/embed endpoint (plural, "input" field) instead of a hand-rolled
// http.Client against the legacy /api/embeddings endpoint.
func TestGenerateVectorUsesOllamakitEmbedEndpoint(t *testing.T) {
	var gotPath, gotBody string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		gotPath = r.URL.Path
		gotBody = string(body)
		vec := make([]float32, DefaultDimensions)
		for i := range vec {
			vec[i] = float32(i+1) / float32(DefaultDimensions)
		}
		_ = json.NewEncoder(w).Encode(struct {
			Embeddings [][]float32 `json:"embeddings"`
		}{Embeddings: [][]float32{vec}})
	}))
	defer server.Close()

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = server.URL

	result := eg.GenerateVector("transport switch check")

	if result.Source != "ollama" {
		t.Fatalf("expected source ollama, got %q", result.Source)
	}
	if result.Model != eg.Model {
		t.Fatalf("expected provenance model %q, got %q", eg.Model, result.Model)
	}
	if len(result.Vector) != DefaultDimensions {
		t.Fatalf("expected %d dimensions, got %d", DefaultDimensions, len(result.Vector))
	}
	if gotPath != "/api/embed" {
		t.Errorf("expected request to ollamakit /api/embed, got path %q", gotPath)
	}
	if !strings.Contains(gotBody, `"input"`) {
		t.Errorf("expected request body with \"input\" field, got %s", gotBody)
	}
	if strings.Contains(gotBody, `"prompt"`) {
		t.Errorf("legacy /api/embeddings style \"prompt\" field must not be used, got %s", gotBody)
	}
}

// TestGenerateVectorUnreachableEndpointFallsBack covers the required
// degradation behavior (#438): when the embedding endpoint is not reachable
// the generator must fall back to the deterministic hash vector with
// unchanged provenance (Source "hash-fallback", Model "").
func TestGenerateVectorUnreachableEndpointFallsBack(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	server.Close() // guaranteed connection refused

	eg := NewEmbeddingsGenerator(nil)
	eg.OllamaURL = server.URL

	result := eg.GenerateVector("no endpoint reachable")

	if result.Source != "hash-fallback" {
		t.Fatalf("expected hash-fallback, got %q", result.Source)
	}
	if result.Model != "" {
		t.Errorf("expected empty model for hash fallback, got %q", result.Model)
	}
	want := GenerateLocalHashVector("no endpoint reachable", DefaultDimensions)
	if len(result.Vector) != len(want) {
		t.Fatalf("expected %d dimensions, got %d", len(want), len(result.Vector))
	}
	for i := range want {
		if result.Vector[i] != want[i] {
			t.Fatalf("hash fallback vector mismatch at index %d", i)
		}
	}
	if got := eg.Metrics().FallbackCount; got != 1 {
		t.Errorf("expected fallback count 1, got %d", got)
	}
}
