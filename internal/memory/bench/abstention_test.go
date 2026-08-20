package bench

import "testing"

func TestComputeAbstention(t *testing.T) {
	queries := []GroundTruth{
		{Query: "answerable high", Answerable: true},
		{Query: "answerable low", Answerable: true},
		{Query: "unanswerable low", Answerable: false},
		{Query: "unanswerable high", Answerable: false},
	}
	topScores := map[int]float64{0: 0.9, 1: 0.1, 2: 0.1, 3: 0.9}

	report := ComputeAbstention("hybrid", 0.5, queries, topScores)
	if report.Total != 4 || report.Correct != 2 {
		t.Fatalf("expected 2/4 correct, got %d/%d", report.Correct, report.Total)
	}
	if report.Accuracy != 0.5 {
		t.Errorf("accuracy = %v, want 0.5", report.Accuracy)
	}

	// Threshold 0: nothing is abstained, so only the answerable queries count.
	report = ComputeAbstention("hybrid", 0, queries, topScores)
	if report.Correct != 2 {
		t.Errorf("threshold 0 must never abstain, correct = %d, want 2", report.Correct)
	}

	// Query with no retrieval at all (score 0) abstains at any positive threshold.
	report = ComputeAbstention("hybrid", 0.5, []GroundTruth{{Answerable: false}}, map[int]float64{})
	if report.Correct != 1 {
		t.Errorf("no results + unanswerable must be a correct abstention, got %d", report.Correct)
	}
}

func TestComputeMetricsSkipsUnanswerableQueries(t *testing.T) {
	gt := []GroundTruth{
		{Query: "q1", RelevantIDs: []string{"a"}, Answerable: true},
		{Query: "q2", Answerable: false},
	}
	results := map[int][]string{0: {"a"}}
	m := ComputeMetrics("hybrid", results, gt, nil)
	if m.QueryCount != 1 {
		t.Errorf("unanswerable query must be excluded from retrieval metrics, QueryCount = %d, want 1", m.QueryCount)
	}
	if m.RecallAt5 != 1.0 {
		t.Errorf("RecallAt5 = %v, want 1.0 (only the answerable query counts)", m.RecallAt5)
	}
}
