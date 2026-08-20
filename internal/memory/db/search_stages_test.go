package db

import (
	"testing"
	"time"
)

// TestApplyFilters_Stage exercises the trust filter stage directly without a
// seeded database. Regression coverage for #536 (decomposed search stages).
func TestApplyFilters_Stage(t *testing.T) {
	db := &DB{}
	q := &SearchQuery{
		TrustFilter: TrustFilter{ExcludeSuperseded: true, MinConfidence: "high"},
	}

	results := []scoredMemory{
		{m: &Memory{ID: "superseded", SupersededBy: "other", Metadata: map[string]string{"confidence": "high"}}},
		{m: &Memory{ID: "lowconf", Metadata: map[string]string{"confidence": "low"}}},
		{m: &Memory{ID: "pass", Metadata: map[string]string{"confidence": "high"}}},
	}

	filtered := db.applyFilters(q, results)

	if len(filtered) != 1 || filtered[0].m.ID != "pass" {
		ids := make([]string, len(filtered))
		for i, r := range filtered {
			ids[i] = r.m.ID
		}
		t.Fatalf("expected only [pass] to survive, got %v", ids)
	}
}

// TestScoreAndRank_Stage verifies the scoring stage ranks a more similar
// embedding above an orthogonal one and sorts descending.
func TestScoreAndRank_Stage(t *testing.T) {
	db := &DB{} // prefilterEnabled=false
	q := &SearchQuery{
		QueryVec: oneHotVec(0),
		Limit:    10,
		Weights:  DefaultRankingWeights(),
	}
	now := time.Now()

	results := []scoredMemory{
		{m: &Memory{ID: "a", Embedding: oneHotVec(0), CreatedAt: now, Importance: 0.5}},
		{m: &Memory{ID: "b", Embedding: oneHotVec(1), CreatedAt: now, Importance: 0.5}},
	}

	results = db.scoreAndRank(q, results)

	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	if results[0].m.ID != "a" {
		t.Fatalf("expected 'a' (cosine 1.0) to rank first, got %q", results[0].m.ID)
	}
	if results[0].score <= results[1].score {
		t.Fatalf("expected a.score > b.score, got %v vs %v", results[0].score, results[1].score)
	}
}

func oneHotVec(idx int) []float32 {
	v := make([]float32, EmbeddingDim)
	v[idx] = 1.0
	return v
}
