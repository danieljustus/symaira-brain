package bench

import (
	"bytes"
	"encoding/json"
	"testing"
	"time"
)

func TestDefaultCorpus(t *testing.T) {
	corpus := DefaultCorpus()

	if len(corpus.Memories) == 0 {
		t.Fatal("expected non-empty corpus")
	}
	if len(corpus.Queries) == 0 {
		t.Fatal("expected non-empty queries")
	}
	if len(corpus.TemporalSlices) == 0 {
		t.Fatal("expected non-empty temporal slices")
	}
	if len(corpus.ScopeSlices) == 0 {
		t.Fatal("expected non-empty scope slices")
	}

	// Verify all query relevant IDs exist in the corpus
	memByID := make(map[string]bool)
	for _, m := range corpus.Memories {
		memByID[m.ID] = true
	}
	for _, q := range corpus.Queries {
		for _, rid := range q.RelevantIDs {
			if !memByID[rid] {
				t.Errorf("query %q references unknown memory ID %s", q.Query, rid)
			}
		}
	}

	// Verify temporal slices reference valid IDs
	for _, ts := range corpus.TemporalSlices {
		for _, id := range ts.CurrentlyValid {
			if !memByID[id] {
				t.Errorf("temporal slice references unknown ID %s in CurrentlyValid", id)
			}
		}
		for _, id := range ts.Expired {
			if !memByID[id] {
				t.Errorf("temporal slice references unknown ID %s in Expired", id)
			}
		}
	}

	// Verify scope slices reference valid IDs
	for _, ss := range corpus.ScopeSlices {
		for _, id := range ss.ExpectedIDs {
			if !memByID[id] {
				t.Errorf("scope slice references unknown ID %s", id)
			}
		}
	}
}

func TestRecallAtK(t *testing.T) {
	tests := []struct {
		name      string
		retrieved []string
		relevant  []string
		k         int
		expected  float64
	}{
		{
			name:      "perfect recall",
			retrieved: []string{"a", "b", "c"},
			relevant:  []string{"a", "b"},
			k:         3,
			expected:  1.0,
		},
		{
			name:      "partial recall",
			retrieved: []string{"a", "x", "y"},
			relevant:  []string{"a", "b"},
			k:         3,
			expected:  0.5,
		},
		{
			name:      "no recall",
			retrieved: []string{"x", "y", "z"},
			relevant:  []string{"a", "b"},
			k:         3,
			expected:  0.0,
		},
		{
			name:      "k smaller than retrieved",
			retrieved: []string{"a", "b", "c"},
			relevant:  []string{"a", "b"},
			k:         1,
			expected:  0.5,
		},
		{
			name:      "empty relevant set",
			retrieved: []string{"a"},
			relevant:  []string{},
			k:         5,
			expected:  0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			relevant := make(map[string]bool)
			for _, id := range tt.relevant {
				relevant[id] = true
			}
			got := RecallAtK(tt.retrieved, relevant, tt.k)
			if diff := got - tt.expected; diff > 1e-9 || diff < -1e-9 {
				t.Errorf("RecallAtK() = %f, want %f", got, tt.expected)
			}
		})
	}
}

func TestNDCGAtK(t *testing.T) {
	// Test 1: Perfect ranking of uneven relevance should give NDCG=1
	retrieved := []string{"a", "b", "c"}
	relevance := map[string]int{"a": 2, "b": 1, "c": 1}
	ndcg := NDCGAtK(retrieved, relevance, 3)
	if ndcg < 0.99 {
		t.Errorf("expected NDCG ~1.0 for perfect ranking, got %f", ndcg)
	}

	// Test 2: Reversed ranking (most relevant last) should give lower NDCG
	retrieved2 := []string{"c", "b", "a"}
	ndcg2 := NDCGAtK(retrieved2, relevance, 3)
	if ndcg2 >= ndcg {
		t.Errorf("reversed ranking should have lower NDCG: %f >= %f", ndcg2, ndcg)
	}

	// Test 3: No relevant docs should give NDCG=0
	retrieved3 := []string{"x", "y", "z"}
	relevance3 := map[string]int{"a": 1, "b": 1}
	ndcg3 := NDCGAtK(retrieved3, relevance3, 3)
	if ndcg3 != 0 {
		t.Errorf("expected NDCG=0 for no relevant docs, got %f", ndcg3)
	}
}

func TestMRR(t *testing.T) {
	tests := []struct {
		name      string
		retrieved []string
		relevant  []string
		expected  float64
	}{
		{
			name:      "first rank",
			retrieved: []string{"a", "b", "c"},
			relevant:  []string{"a"},
			expected:  1.0,
		},
		{
			name:      "second rank",
			retrieved: []string{"x", "a", "c"},
			relevant:  []string{"a"},
			expected:  0.5,
		},
		{
			name:      "not found",
			retrieved: []string{"x", "y", "z"},
			relevant:  []string{"a"},
			expected:  0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			relevant := make(map[string]bool)
			for _, id := range tt.relevant {
				relevant[id] = true
			}
			got := MRR(tt.retrieved, relevant)
			if diff := got - tt.expected; diff > 1e-9 || diff < -1e-9 {
				t.Errorf("MRR() = %f, want %f", got, tt.expected)
			}
		})
	}
}

func TestLatencyPercentiles(t *testing.T) {
	// Generate 100 durations from 1ms to 100ms
	durations := make([]time.Duration, 100)
	for i := range durations {
		durations[i] = time.Duration(i+1) * time.Millisecond
	}

	p50, p95 := LatencyPercentiles(durations)

	// P50 should be around 50ms
	if p50 < 49*time.Millisecond || p50 > 51*time.Millisecond {
		t.Errorf("expected P50 around 50ms, got %v", p50)
	}
	// P95 should be around 95ms
	if p95 < 94*time.Millisecond || p95 > 96*time.Millisecond {
		t.Errorf("expected P95 around 95ms, got %v", p95)
	}
}

func TestComputeMetrics(t *testing.T) {
	groundTruth := []GroundTruth{
		{Query: "q1", RelevantIDs: []string{"a", "b"}},
		{Query: "q2", RelevantIDs: []string{"c"}},
	}

	queryResults := map[int][]string{
		0: {"a", "b", "x"}, // both relevant found
		1: {"x", "c"},      // relevant at rank 2
	}

	latencies := []time.Duration{
		1 * time.Millisecond,
		2 * time.Millisecond,
	}

	metrics := ComputeMetrics("test", queryResults, groundTruth, latencies)

	if metrics.Mode != "test" {
		t.Errorf("expected mode 'test', got %q", metrics.Mode)
	}
	if metrics.QueryCount != 2 {
		t.Errorf("expected 2 queries, got %d", metrics.QueryCount)
	}
	// Recall@5 for q1: 2/2=1.0, for q2: 1/1=1.0 → avg 1.0
	if metrics.RecallAt5 != 1.0 {
		t.Errorf("expected Recall@5 = 1.0, got %f", metrics.RecallAt5)
	}
	// MRR for q1: 1/1=1.0, for q2: 1/2=0.5 → avg 0.75
	if metrics.MRR < 0.74 || metrics.MRR > 0.76 {
		t.Errorf("expected MRR ~0.75, got %f", metrics.MRR)
	}
}

func TestDeterministicBenchmark(t *testing.T) {
	var buf bytes.Buffer
	err := Run(&buf, Options{
		Repetitions: 3,
		Output:      "json",
	})
	if err != nil {
		t.Fatalf("benchmark run failed: %v", err)
	}

	var report Report
	if err := json.Unmarshal(buf.Bytes(), &report); err != nil {
		t.Fatalf("failed to parse JSON report: %v", err)
	}

	// Verify report structure
	if report.CorpusSize == 0 {
		t.Error("expected non-zero corpus size")
	}
	if report.QueryCount == 0 {
		t.Error("expected non-zero query count")
	}
	if report.Repetitions != 3 {
		t.Errorf("expected 3 repetitions, got %d", report.Repetitions)
	}

	// Verify metrics are non-negative
	for _, mode := range []RetrievalMetrics{report.BM25, report.Vector, report.Hybrid} {
		if mode.RecallAt5 < 0 {
			t.Errorf("%s: Recall@5 is negative: %f", mode.Mode, mode.RecallAt5)
		}
		if mode.RecallAt10 < 0 {
			t.Errorf("%s: Recall@10 is negative: %f", mode.Mode, mode.RecallAt10)
		}
		if mode.NDCGAt5 < 0 {
			t.Errorf("%s: NDCG@5 is negative: %f", mode.Mode, mode.NDCGAt5)
		}
		if mode.NDCGAt10 < 0 {
			t.Errorf("%s: NDCG@10 is negative: %f", mode.Mode, mode.NDCGAt10)
		}
		if mode.MRR < 0 {
			t.Errorf("%s: MRR is negative: %f", mode.Mode, mode.MRR)
		}
		if mode.P50LatencyMs < 0 {
			t.Errorf("%s: P50 latency is negative: %f", mode.Mode, mode.P50LatencyMs)
		}
		if mode.P95LatencyMs < 0 {
			t.Errorf("%s: P95 latency is negative: %f", mode.Mode, mode.P95LatencyMs)
		}
		if mode.QueryCount == 0 {
			t.Errorf("%s: query count is zero", mode.Mode)
		}
	}

	// Verify temporal and scope reports exist
	if len(report.Temporal) == 0 {
		t.Error("expected temporal validity reports")
	}
	if len(report.Scope) == 0 {
		t.Error("expected scope isolation reports")
	}
}

func TestTextOutput(t *testing.T) {
	var buf bytes.Buffer
	err := Run(&buf, Options{
		Repetitions: 2,
		Output:      "text",
	})
	if err != nil {
		t.Fatalf("benchmark run failed: %v", err)
	}

	output := buf.String()
	if len(output) == 0 {
		t.Fatal("expected non-empty text output")
	}

	// Verify key sections are present
	for _, section := range []string{
		"Symaira Memory Retrieval Benchmark",
		"Recall@5",
		"Temporal Validity",
		"Scope Isolation",
		"Benchmark Complete",
	} {
		if !contains(output, section) {
			t.Errorf("text output missing section %q", section)
		}
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsSubstring(s, substr))
}

func containsSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
