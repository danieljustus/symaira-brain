package db

import "testing"

// TestFilterHybridByMinScore covers FilterHybridByMinScore: filtering below
// threshold, disabled at minScore<=0, boundary equality.
func TestFilterHybridByMinScore(t *testing.T) {
	results := []HybridResult{
		{Memory: &Memory{ID: "a"}, FusedScore: 0.9},
		{Memory: &Memory{ID: "b"}, FusedScore: 0.5},
		{Memory: &Memory{ID: "c"}, FusedScore: 0.3},
	}

	// minScore <= 0 disables filtering.
	got := FilterHybridByMinScore(results, 0)
	if len(got) != 3 {
		t.Errorf("minScore=0 should return input unchanged, got %d results", len(got))
	}

	// Threshold keeps scores >= minScore.
	got = FilterHybridByMinScore(results, 0.5)
	if len(got) != 2 {
		t.Fatalf("expected 2 results above 0.5, got %d", len(got))
	}
	if got[0].Memory.ID != "a" || got[1].Memory.ID != "b" {
		t.Errorf("expected [a b], got [%s %s]", got[0].Memory.ID, got[1].Memory.ID)
	}

	// Boundary: score equal to minScore is kept.
	got = FilterHybridByMinScore(results, 0.3)
	if len(got) != 3 {
		t.Errorf("boundary score should be kept, got %d results", len(got))
	}

	// Empty input stays empty.
	if got := FilterHybridByMinScore(nil, 0.7); len(got) != 0 {
		t.Errorf("expected empty output for empty input, got %d", len(got))
	}
}
