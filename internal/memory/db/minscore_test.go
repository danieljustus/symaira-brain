package db

import "testing"

func resultsWithScores(scores ...float32) []SearchResult {
	out := make([]SearchResult, len(scores))
	for i, s := range scores {
		out[i] = SearchResult{Memory: &Memory{ID: string(rune('a' + i))}, Score: s}
	}
	return out
}

func TestFilterByMinScore_DisabledWhenZeroOrNegative(t *testing.T) {
	in := resultsWithScores(0.1, 0.9)
	if got := FilterByMinScore(in, 0); len(got) != 2 {
		t.Fatalf("minScore=0 must not filter, got %d results", len(got))
	}
	if got := FilterByMinScore(in, -0.5); len(got) != 2 {
		t.Fatalf("negative minScore must not filter, got %d results", len(got))
	}
}

func TestFilterByMinScore_Boundaries(t *testing.T) {
	in := resultsWithScores(0.25, 0.5, 0.75)

	got := FilterByMinScore(in, 0.5)
	if len(got) != 2 {
		t.Fatalf("expected 2 results at threshold 0.5, got %d", len(got))
	}
	if got[0].Score != 0.5 {
		t.Errorf("score exactly at threshold must be kept, first kept score = %v", got[0].Score)
	}

	got = FilterByMinScore(in, 0.75)
	if len(got) != 1 || got[0].Score != 0.75 {
		t.Fatalf("threshold 0.75 must keep only the top result, got %+v", got)
	}

	if got := FilterByMinScore(in, 0.99); len(got) != 0 {
		t.Fatalf("threshold above all scores must empty the result set, got %d", len(got))
	}
}

func TestFilterByMinScore_PreservesOrderAndEmptyInput(t *testing.T) {
	if got := FilterByMinScore(nil, 0.5); len(got) != 0 {
		t.Fatalf("nil input must stay empty, got %d", len(got))
	}
	in := resultsWithScores(0.9, 0.8, 0.7)
	got := FilterByMinScore(in, 0.5)
	for i, want := range []float32{0.9, 0.8, 0.7} {
		if got[i].Score != want {
			t.Errorf("order not preserved at %d: got %v want %v", i, got[i].Score, want)
		}
	}
}
