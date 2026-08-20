package bench

import (
	"testing"
	"time"
)

// sampleSnapshot builds a snapshot whose quality metrics carry per-query
// samples, so CompareSnapshots can use the bootstrap-CI verdict.
func sampleSnapshot(mode string, recall5 float64, samples []float64) SnapshotFile {
	return SnapshotFile{
		SchemaVersion: 1,
		CreatedAt:     time.Now().UTC(),
		CorpusHash:    "hash",
		CorpusName:    "builtin",
		CorpusSize:    50,
		QueryCount:    len(samples),
		Snapshots: []BenchSnapshot{{
			SchemaVersion: 1,
			Mode:          mode,
			RecallAt5:     recall5,
			RecallAt10:    recall5,
			NDCGAt5:       recall5,
			NDCGAt10:      recall5,
			MRR:           recall5,
			Timestamp:     time.Now().UTC(),
			CorpusHash:    "hash",
			CorpusName:    "builtin",
			Samples: QuerySamples{
				RecallAt5:  samples,
				RecallAt10: samples,
				NDCGAt5:    samples,
				NDCGAt10:   samples,
				MRR:        samples,
			},
		}},
	}
}

// recallDelta finds the Delta for one metric across all modes.
func recallDelta(report ComparisonReport, metric string) Delta {
	for _, d := range report.Deltas {
		if d.Metric == metric {
			return d
		}
	}
	return Delta{}
}

func TestCompareSnapshots_CI_NoRegressionWhenCIOverlaps(t *testing.T) {
	// Baseline mean 0.60; current mean 0.58 — a small dip well inside the
	// CI spread. The CI includes 0.60, so no regression is declared even
	// though the point estimate dropped.
	baseline := sampleSnapshot("bm25", 0.60, []float64{
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6,
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6,
	})
	current := sampleSnapshot("bm25", 0.58, []float64{
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6,
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.4,
	})

	report := CompareSnapshots(baseline, current)
	if !report.Passed {
		t.Fatal("overlapping CI must not be a regression")
	}
	d := recallDelta(report, "Recall@5")
	if d.CI95Lo == 0 && d.CI95Hi == 0 {
		t.Fatal("expected CI fields on the delta")
	}
	if d.CI95Hi < d.Baseline {
		t.Errorf("CI [%f, %f] should include the baseline %f", d.CI95Lo, d.CI95Hi, d.Baseline)
	}
}

func TestCompareSnapshots_CI_RegressionWhenCIExcludesBaseline(t *testing.T) {
	// Baseline mean 0.60; current is a tight cluster around 0.10 — the CI
	// excludes 0.60, so the dip is a proven regression.
	baseline := sampleSnapshot("bm25", 0.60, []float64{
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6,
		0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6,
	})
	current := sampleSnapshot("bm25", 0.10, []float64{
		0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1,
		0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1,
	})

	report := CompareSnapshots(baseline, current)
	if report.Passed {
		t.Fatal("CI entirely below the baseline must be a regression")
	}
	d := recallDelta(report, "Recall@5")
	if d.CI95Hi >= d.Baseline {
		t.Errorf("CI [%f, %f] must exclude the baseline %f", d.CI95Lo, d.CI95Hi, d.Baseline)
	}
	if d.MannWhitney > 0.05 {
		t.Errorf("shifted groups should be significant, Mann-Whitney p=%f", d.MannWhitney)
	}
}

func TestCompareSnapshots_CI_SeededDeterministic(t *testing.T) {
	baseline := sampleSnapshot("bm25", 0.60, []float64{0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6, 0.5, 0.7, 0.6})
	current := sampleSnapshot("bm25", 0.45, []float64{0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4, 0.5, 0.4})

	a := CompareSnapshots(baseline, current)
	b := CompareSnapshots(baseline, current)
	if len(a.Deltas) != len(b.Deltas) {
		t.Fatal("delta counts differ between identical comparisons")
	}
	for i := range a.Deltas {
		if a.Deltas[i].CI95Lo != b.Deltas[i].CI95Lo || a.Deltas[i].CI95Hi != b.Deltas[i].CI95Hi {
			t.Errorf("CI differs between identical comparisons at %d: %+v vs %+v",
				i, a.Deltas[i], b.Deltas[i])
		}
	}
}

func TestCompareSnapshots_CI_LegacySnapshotsFallBack(t *testing.T) {
	// Snapshots without samples keep the point-estimate behavior.
	baseline := sampleSnapshot("bm25", 0.60, nil)
	regressed := sampleSnapshot("bm25", 0.50, nil)

	report := CompareSnapshots(baseline, regressed)
	if report.Passed {
		t.Fatal("point-estimate drop must still count as regression without samples")
	}
	d := recallDelta(report, "Recall@5")
	if d.CI95Lo != 0 || d.CI95Hi != 0 {
		t.Errorf("legacy fallback must not emit CI fields, got %+v", d)
	}
}
