package bench

import "testing"

// TestRunAgingAssessmentDeterministic guards the hermetic contract: the
// same seed must produce the same report, so CI snapshots are stable.
func TestRunAgingAssessmentDeterministic(t *testing.T) {
	a, err := RunAgingAssessment(42)
	if err != nil {
		t.Fatal(err)
	}
	b, err := RunAgingAssessment(42)
	if err != nil {
		t.Fatal(err)
	}
	if a != b {
		t.Fatalf("same seed produced different reports:\n%+v\n%+v", a, b)
	}
}

// TestRunAgingAssessmentHelpsRecall asserts the point of the measurement:
// on the issue's exact failure mode — a fact stated once eighteen months
// ago competing with a fact confirmed yesterday — applying the aging decay
// multiplier must improve the confirmed fact's rank and clear stale facts
// from the top-5. This is the "better recall" definition #491 asks for.
func TestRunAgingAssessmentHelpsRecall(t *testing.T) {
	report, err := RunAgingAssessment(42)
	if err != nil {
		t.Fatal(err)
	}
	if report.Queries == 0 {
		t.Fatal("assessment produced no queries")
	}
	if report.MRRDelta <= 0 {
		t.Errorf("aging must improve MRR of the confirmed fact on a stale-heavy store, got delta %+.4f", report.MRRDelta)
	}
	if report.StaleCrowdDelta <= 0 {
		t.Errorf("aging must clear stale facts from the top-5, got crowding delta %+.4f", report.StaleCrowdDelta)
	}
}
