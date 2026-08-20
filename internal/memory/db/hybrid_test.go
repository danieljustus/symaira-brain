package db

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestTokenize(t *testing.T) {
	tokens := Tokenize("Hello, World! This is a test.")
	if len(tokens) != 3 {
		t.Errorf("expected 3 tokens (hello, world, test), got %d: %v", len(tokens), tokens)
	}
}

func TestTokenize_StopWordsRemoved(t *testing.T) {
	tokens := Tokenize("the quick brown fox is a very fast animal")
	for _, stop := range []string{"the", "is", "a", "very"} {
		for _, tok := range tokens {
			if tok == stop {
				t.Errorf("stop word %q should be removed", stop)
			}
		}
	}
}

func TestReciprocalRankFusion_MergesResults(t *testing.T) {
	list1 := []string{"a", "b", "c"}
	list2 := []string{"b", "c", "d"}

	scores := ReciprocalRankFusion([][]string{list1, list2}, 60)
	if scores["b"] <= scores["a"] {
		t.Errorf("expected 'b' (in both lists) to score higher than 'a' (in one list)")
	}
	if scores["b"] <= scores["d"] {
		t.Errorf("expected 'b' to score higher than 'd'")
	}
}

func TestSearchMemoriesBM25_FindsExactKeyword(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	dark := &Memory{
		ID:       "dark-mode",
		Content:  "Alice prefers dark mode in all applications",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(dark); err != nil {
		t.Fatal(err)
	}

	light := &Memory{
		ID:       "light-theme",
		Content:  "Bob likes light themes for everything",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(light); err != nil {
		t.Fatal(err)
	}

	results, err := database.SearchMemoriesBM25("dark mode", "global", 5)
	if err != nil {
		t.Fatal(err)
	}
	if len(results) == 0 {
		t.Fatal("expected at least 1 result")
	}
	if results[0].Memory.ID != "dark-mode" {
		t.Errorf("expected dark-mode as top result, got %s", results[0].Memory.ID)
	}
}

func TestSearchMemoriesBM25_EmptyQuery(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	results, err := database.SearchMemoriesBM25("the a is", "global", 5)
	if err != nil {
		t.Fatal(err)
	}
	if results != nil {
		t.Errorf("expected nil for stop-word-only query, got %d results", len(results))
	}
}

func TestSearchMemoriesBM25_RejectsInvalidScope(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	injections := []string{
		`" OR 1=1 --`,
		"global OR NOT global",
		"scope:*",
		"global NEAR test",
	}
	for _, scope := range injections {
		_, err := database.SearchMemoriesBM25("test", scope, 5)
		if err == nil {
			t.Errorf("expected error for injected scope %q, got nil", scope)
		}
	}
}

func TestHybridSearch_CandidateCap(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	const count = maxCandidates + 500
	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()

	for i := 0; i < count; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		emb[(i+37)%EmbeddingDim] = 0.5
		m := &Memory{
			ID:        fmt.Sprintf("cap-mem-%d", i),
			Content:   fmt.Sprintf("test document number %d for candidate cap", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			t.Fatalf("failed to save memory %d: %v", i, err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}

	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0

	highLimit := maxCandidates / 2
	results, err := database.HybridSearch(queryVec, "", "test document", "global", highLimit, "", TrustFilter{}, PolicyFilter{}, 0.5, 0.5)
	if err != nil {
		t.Fatalf("HybridSearch failed: %v", err)
	}
	if len(results) > highLimit {
		t.Errorf("expected at most %d results, got %d", highLimit, len(results))
	}
}

func TestHybridSearch_SmallLimitUnchanged(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	for i := 0; i < 50; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		m := &Memory{
			ID:        fmt.Sprintf("small-mem-%d", i),
			Content:   fmt.Sprintf("document %d about testing", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to save memory %d: %v", i, err)
		}
	}

	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0

	results, err := database.HybridSearch(queryVec, "", "testing", "global", 10, "", TrustFilter{}, PolicyFilter{}, 0.5, 0.5)
	if err != nil {
		t.Fatalf("HybridSearch failed: %v", err)
	}
	if len(results) > 10 {
		t.Errorf("expected at most 10 results, got %d", len(results))
	}
}

func TestSparsemax_ProbabilitiesSumToOne(t *testing.T) {
	scores := []float64{2.0, 1.5, 0.5, 0.1}
	sparse := Sparsemax(scores)

	// Verify output length matches input
	if len(sparse) != len(scores) {
		t.Fatalf("expected %d results, got %d", len(scores), len(sparse))
	}

	// Verify sum ≈ 1.0
	var sum float64
	for _, v := range sparse {
		sum += v
	}
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("sparsemax probabilities should sum to ~1.0, got %f", sum)
	}

	// Some elements should be zero (sparse property)
	nonZero := 0
	for _, v := range sparse {
		if v > 0 {
			nonZero++
		}
	}
	if nonZero >= len(scores) {
		t.Errorf("expected some zero entries for sparsemax, all %d were non-zero", nonZero)
	}
	if nonZero == 0 {
		t.Errorf("expected at least one non-zero entry")
	}
}

func TestSparsemax_AllEqualValues(t *testing.T) {
	scores := []float64{1.0, 1.0, 1.0}
	sparse := Sparsemax(scores)

	var sum float64
	for _, v := range sparse {
		sum += v
	}
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("sparsemax(all equal) should sum to 1, got %f", sum)
	}
	for _, v := range sparse {
		if v <= 0 {
			t.Errorf("with all equal values, every element should be positive, got %f", v)
		}
	}
}

func TestSparsemax_NegativeScores(t *testing.T) {
	scores := []float64{-1.0, -0.5, 0.0, 0.5, 1.0}
	sparse := Sparsemax(scores)

	var sum float64
	for _, v := range sparse {
		sum += v
	}
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("sparsemax(negatives) should sum to 1, got %f", sum)
	}
	// At least the negative scores should be zero
	if sparse[0] != 0 || sparse[1] != 0 {
		t.Errorf("negative scores should be zeroed by sparsemax, got %v", sparse)
	}
}

func TestSparsemax_EmptyInput(t *testing.T) {
	if got := Sparsemax(nil); got != nil {
		t.Errorf("expected nil for empty input, got %v", got)
	}
	if got := Sparsemax([]float64{}); got != nil {
		t.Errorf("expected nil for empty input, got %v", got)
	}
}

func TestSparsemax_SingleElement(t *testing.T) {
	sparse := Sparsemax([]float64{3.0})
	if len(sparse) != 1 {
		t.Fatalf("expected 1 element, got %d", len(sparse))
	}
	if sparse[0] < 0.99 || sparse[0] > 1.01 {
		t.Errorf("single element should map to 1.0, got %f", sparse[0])
	}
}

func TestSparsemax_ZeroVector(t *testing.T) {
	scores := []float64{0, 0, 0, 0}
	sparse := Sparsemax(scores)
	var sum float64
	for _, v := range sparse {
		sum += v
	}
	if sum < 0.99 || sum > 1.01 {
		t.Errorf("sparsemax(zero vector) should sum to 1, got %f", sum)
	}
	// All zeros: τ = (0-1)/4 = -0.25, so all get max(0, 0-(-0.25)) = 0.25
	for i, v := range sparse {
		if v < 0.24 || v > 0.26 {
			t.Errorf("element %d: expected ~0.25, got %f", i, v)
		}
	}
}

func TestTrackMemoryAccessBatchWritesTimestamp(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	ids := []string{"acc-1", "acc-2", "acc-3"}
	for _, id := range ids {
		m := &Memory{
			ID:        id,
			Content:   "access tracking " + id,
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: make([]float32, EmbeddingDim),
		}
		if err := database.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}

	if err := database.TrackMemoryAccessBatch(ids); err != nil {
		t.Fatalf("TrackMemoryAccessBatch failed: %v", err)
	}

	// Regression guard: last_access must hold a real timestamp that scans as
	// sql.NullTime, not one of the IDs (a swapped-argument bug once wrote the
	// first ID into last_access, which broke every later scan).
	for _, id := range ids {
		var ac int
		var la sql.NullTime
		if err := database.Conn().QueryRow(
			"SELECT access_count, last_access FROM memories WHERE id = ?", id,
		).Scan(&ac, &la); err != nil {
			t.Fatalf("read back %s failed: %v", id, err)
		}
		if ac != 2 {
			t.Errorf("%s: expected access_count 2 after batch (1 from save default + 1), got %d", id, ac)
		}
		if !la.Valid {
			t.Errorf("%s: expected last_access timestamp after batch, got NULL", id)
		}
	}
}
