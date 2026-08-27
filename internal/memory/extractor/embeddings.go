package extractor

import (
	"context"
	"crypto/sha256"
	"fmt"
	"hash/fnv"
	"math"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-corekit/llmkit"
	lru "github.com/hashicorp/golang-lru/v2"
)

// EmbeddingsGenerator coordinates local and cloud-fallback embedding generation.
type EmbeddingsGenerator struct {
	OllamaURL      string
	Model          string
	OllamaTimeout  time.Duration
	mu             sync.Mutex
	lastFail       time.Time
	embeddingCache *lru.Cache[string, []float32]

	// ollama is the shared llmkit transport (no hand-rolled http.Client
	// here, see #438). It is rebuilt lazily when OllamaURL or OllamaTimeout
	// change so callers can reconfigure the generator after construction.
	ollama           *llmkit.Client
	ollamaBaseURL    string
	ollamaTimeoutSet time.Duration

	// Metrics
	ollamaHits      atomic.Int64
	ollamaMisses    atomic.Int64
	fallbackCount   atomic.Int64
	totalRequests   atomic.Int64
	ollamaLatencyNs atomic.Int64
}

const (
	DefaultDimensions    = 768
	ollamaCacheTTL       = 30 * time.Second
	defaultOllamaTimeout = 2 * time.Second
)

// NewEmbeddingsGenerator configures an embeddings generator from the
// supplied config. The caller (typically cmd/) is responsible for
// loading configuration via config.Load(); this package never reads
// config files directly. When cfg is nil, hardcoded defaults are used.
func NewEmbeddingsGenerator(cfg *config.Config) *EmbeddingsGenerator {
	if cfg == nil {
		cfg = config.Defaults()
	}
	ollamaURL := "http://localhost:11434/api/embeddings"
	model := "nomic-embed-text"
	if cfg.Ollama.URL != "" {
		ollamaURL = cfg.Ollama.URL
	}
	if cfg.Ollama.Model != "" {
		model = cfg.Ollama.Model
	}

	cache, _ := lru.New[string, []float32](10000)

	return &EmbeddingsGenerator{
		OllamaURL:      ollamaURL,
		Model:          model,
		OllamaTimeout:  defaultOllamaTimeout,
		embeddingCache: cache,
	}
}

// EmbeddingResult carries the generated vector together with provenance
// metadata that identifies which embedding space the vector belongs to.
// Search and consolidation must never cross-score rows from different sources.
type EmbeddingResult struct {
	Vector       []float32 // the embedding vector
	Source       string    // "ollama" or "hash-fallback"
	Model        string    // model name (e.g. "nomic-embed-text") or "" for hash
	Quantization string    // quantization level of the embedding space ("" = unquantized)
}

// GenerateVector produces a 768-dimensional vector using Ollama if available,
// or the local hashing fallback. Ollama vectors are cached by content hash to
// avoid redundant computation for identical text. Fallback vectors are never
// cached so that recovery after Ollama comes back online is automatic.
func (eg *EmbeddingsGenerator) GenerateVector(text string) EmbeddingResult {
	return eg.GenerateVectorWithContext(context.Background(), text)
}

// GenerateVectorWithContext is like GenerateVector but respects the provided
// context for timeout control. If the context expires before Ollama responds,
// the request falls back to hash-based embeddings automatically.
func (eg *EmbeddingsGenerator) GenerateVectorWithContext(ctx context.Context, text string) EmbeddingResult {
	eg.totalRequests.Add(1)

	cacheKey := eg.cacheKey(text)
	if cached, ok := eg.embeddingCache.Get(cacheKey); ok {
		eg.ollamaHits.Add(1)
		return EmbeddingResult{Vector: cached, Source: "ollama", Model: eg.Model}
	}

	dims := DefaultDimensions

	eg.mu.Lock()
	skip := time.Since(eg.lastFail) < ollamaCacheTTL
	eg.mu.Unlock()

	if !skip {
		start := time.Now()
		vec, err := eg.queryOllamaWithContext(ctx, text)
		latency := time.Since(start)
		eg.ollamaLatencyNs.Add(latency.Nanoseconds())

		if err == nil && len(vec) == dims {
			eg.embeddingCache.Add(cacheKey, vec)
			eg.ollamaHits.Add(1)
			return EmbeddingResult{Vector: vec, Source: "ollama", Model: eg.Model}
		}
		eg.mu.Lock()
		eg.lastFail = time.Now()
		eg.mu.Unlock()
		eg.ollamaMisses.Add(1)
	}

	eg.fallbackCount.Add(1)
	vec := GenerateLocalHashVector(text, dims)
	return EmbeddingResult{Vector: vec, Source: "hash-fallback", Model: ""}
}

// ActiveBackend reports the embedding backend that would be used for the
// next GenerateVector call. It returns "ollama" when Ollama is reachable or
// the cooldown has expired, and "lexical" when Ollama is currently being
// skipped due to a recent failure within the cooldown window.
func (eg *EmbeddingsGenerator) ActiveBackend() string {
	eg.mu.Lock()
	defer eg.mu.Unlock()
	if time.Since(eg.lastFail) < ollamaCacheTTL {
		return "lexical"
	}
	return "ollama"
}

// Dimensions returns the vector dimension count produced by this generator.
func (eg *EmbeddingsGenerator) Dimensions() int {
	return DefaultDimensions
}

// MarkOllamaFailed records an Ollama failure, switching the generator to
// lexical-fallback mode for the duration of the cooldown window. This is
// useful in tests that need to exercise the fallback path.
func (eg *EmbeddingsGenerator) MarkOllamaFailed() {
	eg.mu.Lock()
	eg.lastFail = time.Now()
	eg.mu.Unlock()
}

// Metrics returns the current embedding generation metrics.
func (eg *EmbeddingsGenerator) Metrics() EmbeddingMetrics {
	total := eg.totalRequests.Load()
	var avgLatencyMs float64
	ollamaReqs := eg.ollamaHits.Load() + eg.ollamaMisses.Load()
	if ollamaReqs > 0 {
		avgLatencyMs = float64(eg.ollamaLatencyNs.Load()) / float64(ollamaReqs) / 1e6
	}
	var fallbackRate float64
	if total > 0 {
		fallbackRate = float64(eg.fallbackCount.Load()) / float64(total)
	}
	return EmbeddingMetrics{
		TotalRequests: total,
		OllamaHits:    eg.ollamaHits.Load(),
		OllamaMisses:  eg.ollamaMisses.Load(),
		FallbackCount: eg.fallbackCount.Load(),
		FallbackRate:  fallbackRate,
		AvgOllamaMs:   avgLatencyMs,
	}
}

// EmbeddingMetrics holds metrics for embedding generation.
type EmbeddingMetrics struct {
	TotalRequests int64
	OllamaHits    int64
	OllamaMisses  int64
	FallbackCount int64
	FallbackRate  float64
	AvgOllamaMs   float64
}

func (eg *EmbeddingsGenerator) cacheKey(text string) string {
	h := sha256.Sum256([]byte(text))
	return fmt.Sprintf("%x", h[:16])
}

// ollamaClient returns the shared llmkit transport, rebuilding it when
// OllamaURL or OllamaTimeout changed since the last call. Timeout and retry
// behavior is configured deliberately rather than inherited: a single
// attempt with an explicit OllamaTimeout (default 2s, matching the legacy
// embedding path) is used, and no retries are performed — embedding calls
// are cheap to recompute, and the failure cooldown (ollamaCacheTTL) plus
// the deterministic hash fallback already absorb transient Ollama outages.
// The caller's context deadline layers on top of the client timeout, so the
// shorter of the two wins.
func (eg *EmbeddingsGenerator) ollamaClient() (*llmkit.Client, error) {
	base := ollamaBaseURL(eg.OllamaURL)
	timeout := eg.OllamaTimeout
	if timeout <= 0 {
		timeout = defaultOllamaTimeout
	}

	eg.mu.Lock()
	defer eg.mu.Unlock()
	if eg.ollama == nil || eg.ollamaBaseURL != base || eg.ollamaTimeoutSet != timeout {
		desc, ok := llmkit.Lookup("ollama")
		if !ok {
			return nil, fmt.Errorf("ollama descriptor missing from embedded registry")
		}
		// The registry descriptor already carries the OpenAI-compatible
		// /v1 path; normalize a configured bare host onto it as well.
		baseV1 := base
		if !strings.HasSuffix(baseV1, "/v1") {
			baseV1 = strings.TrimRight(baseV1, "/") + "/v1"
		}
		cl, err := llmkit.NewClient(desc, "", llmkit.WithBaseURL(baseV1), llmkit.WithTimeout(timeout))
		if err != nil {
			return nil, err
		}
		eg.ollama = cl
		eg.ollamaBaseURL = base
		eg.ollamaTimeoutSet = timeout
	}
	return eg.ollama, nil
}

// ollamaBaseURL strips a configured Ollama endpoint URL (e.g.
// "http://localhost:11434/api/embeddings") down to the scheme+host root
// that the llmkit ollama descriptor expects (the OpenAI-compatible /v1
// path is applied by NewClient's base-URL handling). Malformed input is
// passed through unchanged so the descriptor's own defaulting takes over.
func ollamaBaseURL(raw string) string {
	// The configured URL historically named the native /api/embeddings
	// endpoint; the shared transport expects a bare scheme+host, so strip
	// any path component ("/api/embeddings", "/api/embed", "/v1", ...).
	for i := 0; i < len(raw); i++ {
		if raw[i] == '/' && i > len("http://") {
			return raw[:i]
		}
	}
	return raw
}

func (eg *EmbeddingsGenerator) queryOllamaWithContext(ctx context.Context, text string) ([]float32, error) {
	// The single Ollama call goes through the shared llmkit transport
	// (corekit v0.15.0) — the same client family internal/llm uses —
	// instead of a hand-rolled http.Client (see #438). llmkit posts to the
	// OpenAI-compatible /v1/embeddings endpoint; for the same model and
	// input text the resulting vector is equivalent to the legacy
	// /api/embeddings response, so no reindex is required. Any error
	// (unreachable host, timeout, non-2xx) is returned as-is; the caller
	// degrades to the hash fallback.
	cl, err := eg.ollamaClient()
	if err != nil {
		return nil, err
	}
	embeddings, err := cl.Embed(ctx, eg.Model, []string{text})
	if err != nil {
		return nil, err
	}
	return embeddings[0].Vector, nil
}

// GenerateLocalHashVector utilizes the "Hashing Trick" to produce a normalized 768-dim vector in microseconds.
func GenerateLocalHashVector(text string, dimensions int) []float32 {
	vec := make([]float32, dimensions)

	// Normalize and tokenize text
	cleaned := strings.ToLower(text)
	// Replace punctuation with spaces
	for _, char := range []string{".", ",", "!", "?", ";", ":", "-", "_", "(", ")", "[", "]", "{", "}"} {
		cleaned = strings.ReplaceAll(cleaned, char, " ")
	}

	words := strings.Fields(cleaned)
	if len(words) == 0 {
		return vec
	}

	// Distribute word hashes across vector dimensions
	for _, word := range words {
		if isStopWord(word) {
			continue
		}

		h := fnv.New32a()
		h.Write([]byte(word))
		hashVal := h.Sum32()

		idx := int(hashVal) % dimensions

		// Add weighting based on hash signature
		vec[idx] += 1.0
	}

	// Normalize the vector (L2 norm) so cosine similarity behaves correctly
	var sumSquares float64
	for _, val := range vec {
		sumSquares += float64(val * val)
	}

	if sumSquares > 0 {
		norm := float32(math.Sqrt(sumSquares))
		for i := range vec {
			vec[i] /= norm
		}
	}

	return vec
}

func isStopWord(w string) bool {
	// Standard compact list of English and German stop words
	stops := map[string]bool{
		"and": true, "the": true, "a": true, "an": true, "of": true, "to": true, "in": true, "is": true, "it": true, "that": true,
		"und": true, "der": true, "die": true, "das": true, "ein": true, "eine": true, "ist": true, "es": true, "dass": true,
		"von": true, "zu": true, "mit": true, "auf": true, "für": true, "den": true, "dem": true, "des": true, "im": true, "am": true,
	}
	return stops[w]
}
