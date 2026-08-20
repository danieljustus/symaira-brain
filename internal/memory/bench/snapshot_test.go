package bench

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestComputeCorpusHash_Deterministic(t *testing.T) {
	c1 := DefaultCorpus()
	c2 := DefaultCorpus()

	h1 := ComputeCorpusHash(c1)
	h2 := ComputeCorpusHash(c2)

	if h1 != h2 {
		t.Errorf("corpus hash should be deterministic, got %q vs %q", h1, h2)
	}
	if len(h1) != 64 {
		t.Errorf("expected 64-char SHA-256 hex, got %d chars: %s", len(h1), h1)
	}
}

func TestCorpusHash_ChangesWithCorpus(t *testing.T) {
	base := DefaultCorpus()
	h1 := ComputeCorpusHash(base)

	// Modify a memory content
	modified := DefaultCorpus()
	modified.Memories[0].Content = "modified content"
	h2 := ComputeCorpusHash(modified)

	if h1 == h2 {
		t.Error("corpus hash should change when memory content changes")
	}
}

func TestSnapshotRoundTrip(t *testing.T) {
	corpus := DefaultCorpus()
	corpusHash := ComputeCorpusHash(corpus)

	report := Report{
		Timestamp:       time.Now().UTC().Truncate(time.Second),
		CorpusSize:      len(corpus.Memories),
		QueryCount:      len(corpus.Queries),
		Repetitions:     5,
		EmbeddingSource: "hash-fallback",
		BM25: RetrievalMetrics{
			Mode: "bm25", RecallAt5: 0.85, RecallAt10: 0.90,
			NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92,
			MeanLatencyMs: 0.5, P50LatencyMs: 0.4, P95LatencyMs: 1.0,
		},
		Vector: RetrievalMetrics{
			Mode: "vector", RecallAt5: 0.75, RecallAt10: 0.82,
			NDCGAt5: 0.72, NDCGAt10: 0.79, MRR: 0.80,
			MeanLatencyMs: 2.0, P50LatencyMs: 1.8, P95LatencyMs: 4.5,
		},
		Hybrid: RetrievalMetrics{
			Mode: "hybrid", RecallAt5: 0.90, RecallAt10: 0.95,
			NDCGAt5: 0.88, NDCGAt10: 0.92, MRR: 0.95,
			MeanLatencyMs: 1.2, P50LatencyMs: 1.0, P95LatencyMs: 3.0,
		},
	}

	snap := report.ToSnapshotFile("builtin", corpusHash)

	if snap.SchemaVersion != 1 {
		t.Errorf("expected schema_version=1, got %d", snap.SchemaVersion)
	}
	if snap.CorpusHash != corpusHash {
		t.Errorf("expected corpus_hash=%q, got %q", corpusHash, snap.CorpusHash)
	}
	if len(snap.Snapshots) != 3 {
		t.Errorf("expected 3 mode snapshots, got %d", len(snap.Snapshots))
	}
	if snap.Snapshots[0].Mode != "bm25" {
		t.Errorf("first snapshot should be bm25, got %s", snap.Snapshots[0].Mode)
	}

	// Round-trip through disk
	tmpDir := t.TempDir()
	snapDir := filepath.Join(tmpDir, SnapshotsDir)
	path, err := SaveSnapshot(snapDir, snap)
	if err != nil {
		t.Fatalf("SaveSnapshot failed: %v", err)
	}

	if _, err := os.Stat(path); os.IsNotExist(err) {
		t.Fatalf("snapshot file not created at %s", path)
	}

	loaded, err := LoadSnapshot(path)
	if err != nil {
		t.Fatalf("LoadSnapshot failed: %v", err)
	}

	if loaded.SchemaVersion != 1 {
		t.Errorf("loaded schema_version mismatch: %d", loaded.SchemaVersion)
	}
	if loaded.CorpusHash != corpusHash {
		t.Errorf("loaded corpus_hash mismatch: %q", loaded.CorpusHash)
	}
	if len(loaded.Snapshots) != 3 {
		t.Errorf("loaded %d snapshots, want 3", len(loaded.Snapshots))
	}
	if loaded.Snapshots[0].MRR != 0.92 {
		t.Errorf("loaded bm25 MRR mismatch: %f", loaded.Snapshots[0].MRR)
	}
}

func TestFindBaselineSnapshot(t *testing.T) {
	corpus := DefaultCorpus()
	hash := ComputeCorpusHash(corpus)

	// No baseline yet — expect empty result, not error
	snapDir := t.TempDir()
	snap, path, err := FindBaselineSnapshot(snapDir, "builtin", hash)
	if err != nil {
		t.Fatalf("FindBaselineSnapshot returned unexpected error for missing baseline: %v", err)
	}
	if snap.SchemaVersion != 0 {
		t.Error("expected zero-value snapshot for missing baseline")
	}
	if path != "" {
		t.Errorf("expected empty path for missing baseline, got %q", path)
	}

	// Save a baseline and find it
	report := Report{
		Timestamp:  time.Now().UTC().Truncate(time.Second),
		CorpusSize: 50, QueryCount: 12, Repetitions: 5,
		BM25:   RetrievalMetrics{Mode: "bm25", RecallAt5: 0.9},
		Vector: RetrievalMetrics{Mode: "vector", RecallAt5: 0.8},
		Hybrid: RetrievalMetrics{Mode: "hybrid", RecallAt5: 0.95},
	}
	snap = report.ToSnapshotFile("builtin", hash)
	if _, err := SaveSnapshot(snapDir, snap); err != nil {
		t.Fatalf("SaveSnapshot failed: %v", err)
	}

	found, foundPath, err := FindBaselineSnapshot(snapDir, "builtin", hash)
	if err != nil {
		t.Fatalf("FindBaselineSnapshot failed after save: %v", err)
	}
	if found.SchemaVersion != 1 {
		t.Error("expected found baseline after save")
	}
	if foundPath == "" {
		t.Error("expected non-empty path for found baseline")
	}
}

func TestCompareSnapshots_Pass(t *testing.T) {
	corpus := DefaultCorpus()
	hash := ComputeCorpusHash(corpus)
	ts := time.Now().UTC().Truncate(time.Second)

	baseline := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.85, RecallAt10: 0.90, NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92, P50LatencyMs: 0.5, P95LatencyMs: 1.0},
			{SchemaVersion: 1, Mode: "vector", RecallAt5: 0.75, RecallAt10: 0.82, NDCGAt5: 0.72, NDCGAt10: 0.79, MRR: 0.80, P50LatencyMs: 2.0, P95LatencyMs: 4.5},
			{SchemaVersion: 1, Mode: "hybrid", RecallAt5: 0.90, RecallAt10: 0.95, NDCGAt5: 0.88, NDCGAt10: 0.92, MRR: 0.95, P50LatencyMs: 1.2, P95LatencyMs: 3.0},
		},
	}

	// All metrics improved
	current := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts.Add(time.Hour), CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.87, RecallAt10: 0.92, NDCGAt5: 0.84, NDCGAt10: 0.90, MRR: 0.93, P50LatencyMs: 0.4, P95LatencyMs: 0.9},
			{SchemaVersion: 1, Mode: "vector", RecallAt5: 0.77, RecallAt10: 0.84, NDCGAt5: 0.74, NDCGAt10: 0.81, MRR: 0.82, P50LatencyMs: 1.8, P95LatencyMs: 4.0},
			{SchemaVersion: 1, Mode: "hybrid", RecallAt5: 0.92, RecallAt10: 0.96, NDCGAt5: 0.90, NDCGAt10: 0.94, MRR: 0.96, P50LatencyMs: 1.0, P95LatencyMs: 2.8},
		},
	}

	comp := CompareSnapshots(baseline, current)
	if !comp.Passed {
		t.Error("expected all metrics to pass when everything improved")
	}
	if len(comp.Deltas) != 3*7 { // 3 modes × 7 metrics
		t.Errorf("expected %d deltas, got %d", 21, len(comp.Deltas))
	}

	for _, d := range comp.Deltas {
		if !d.Better {
			t.Errorf("expected metric %s/%s to be better: Δ=%+.4f", d.Mode, d.Metric, d.Delta)
		}
	}
}

func TestCompareSnapshots_Regression_Recall(t *testing.T) {
	corpus := DefaultCorpus()
	hash := ComputeCorpusHash(corpus)
	ts := time.Now().UTC().Truncate(time.Second)

	baseline := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.85, RecallAt10: 0.90, NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92, P50LatencyMs: 0.5, P95LatencyMs: 1.0},
			{SchemaVersion: 1, Mode: "vector", RecallAt5: 0.75, RecallAt10: 0.82, NDCGAt5: 0.72, NDCGAt10: 0.79, MRR: 0.80, P50LatencyMs: 2.0, P95LatencyMs: 4.5},
			{SchemaVersion: 1, Mode: "hybrid", RecallAt5: 0.90, RecallAt10: 0.95, NDCGAt5: 0.88, NDCGAt10: 0.92, MRR: 0.95, P50LatencyMs: 1.2, P95LatencyMs: 3.0},
		},
	}

	// Recall regressed in bm25
	regressed := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts.Add(2 * time.Hour), CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.70, RecallAt10: 0.75, NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92, P50LatencyMs: 0.5, P95LatencyMs: 1.0},
			{SchemaVersion: 1, Mode: "vector", RecallAt5: 0.75, RecallAt10: 0.82, NDCGAt5: 0.72, NDCGAt10: 0.79, MRR: 0.80, P50LatencyMs: 2.0, P95LatencyMs: 4.5},
			{SchemaVersion: 1, Mode: "hybrid", RecallAt5: 0.90, RecallAt10: 0.95, NDCGAt5: 0.88, NDCGAt10: 0.92, MRR: 0.95, P50LatencyMs: 1.2, P95LatencyMs: 3.0},
		},
	}

	comp := CompareSnapshots(baseline, regressed)
	if comp.Passed {
		t.Error("expected failed comparison when recall regressed")
	}

	// Verify the bm25 Recall@5 delta is calculated correctly
	var bm25Recall5Delta *Delta
	for _, d := range comp.Deltas {
		if d.Mode == "bm25" && d.Metric == "Recall@5" {
			bm25Recall5Delta = &d
			break
		}
	}
	if bm25Recall5Delta == nil {
		t.Fatal("expected bm25/Recall@5 delta")
	}
	if bm25Recall5Delta.Delta >= 0 {
		t.Errorf("expected negative delta for recall regression, got %f", bm25Recall5Delta.Delta)
	}
	if bm25Recall5Delta.Better {
		t.Error("expected recall regression to not be better")
	}
}

func TestCompareSnapshots_Regression_Latency(t *testing.T) {
	corpus := DefaultCorpus()
	hash := ComputeCorpusHash(corpus)
	ts := time.Now().UTC().Truncate(time.Second)

	baseline := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.85, RecallAt10: 0.90, NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92, P50LatencyMs: 0.5, P95LatencyMs: 1.0},
		},
	}

	// Latency regressed (increased) — quality metrics unchanged
	slower := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts.Add(3 * time.Hour), CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.85, RecallAt10: 0.90, NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92, P50LatencyMs: 5.0, P95LatencyMs: 10.0},
		},
	}

	comp := CompareSnapshots(baseline, slower)
	if comp.Passed {
		t.Error("expected failed comparison when latency regressed")
	}

	// Verify P50 delta
	var p50Delta *Delta
	for _, d := range comp.Deltas {
		if d.Mode == "bm25" && d.Metric == "P50 Latency (ms)" {
			p50Delta = &d
			break
		}
	}
	if p50Delta == nil {
		t.Fatal("expected bm25/P50 Latency (ms) delta")
	}
	if p50Delta.Better {
		t.Error("expected latency increase to not be better (lower is better)")
	}
	expectedDelta := 5.0 - 0.5
	if p50Delta.Delta != expectedDelta {
		t.Errorf("expected delta %f, got %f", expectedDelta, p50Delta.Delta)
	}
}

func TestCompareSnapshots_NoMissingMode(t *testing.T) {
	ts := time.Now().UTC().Truncate(time.Second)
	hash := "abcdef1234567890abcdef1234567890abcdef12"

	// Current has modes not in baseline — should skip gracefully
	baseline := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "test",
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.80},
		},
	}
	current := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "test",
		Snapshots: []BenchSnapshot{
			{SchemaVersion: 1, Mode: "bm25", RecallAt5: 0.85},
			{SchemaVersion: 1, Mode: "unknown", RecallAt5: 0.90},
		},
	}

	comp := CompareSnapshots(baseline, current)
	// Should have 7 deltas (only bm25 matched — unknown skipped)
	if len(comp.Deltas) != 7 {
		t.Errorf("expected 7 deltas (only bm25 matched), got %d", len(comp.Deltas))
	}
	if !comp.Passed {
		t.Error("expected pass when all matching metrics improved")
	}
}

func TestSnapshotFileName(t *testing.T) {
	name := SnapshotFileName("builtin", "abcdef1234567890")
	if name != "bench-snapshot-builtin-abcdef123456.json" {
		t.Errorf("unexpected filename: %s", name)
	}

	// Short hash
	name2 := SnapshotFileName("longmemeval", "abc")
	if name2 != "bench-snapshot-longmemeval-abc.json" {
		t.Errorf("unexpected filename for short hash: %s", name2)
	}
}

// comparisonFixture builds a comparison of a single bm25 snapshot between a
// baseline and a current run, letting tests vary quality and latency.
func comparisonFixture(baselineRecall, currentRecall, currentP50 float64) ComparisonReport {
	ts := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	const hash = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899"
	mk := func(recall, p50 float64) BenchSnapshot {
		return BenchSnapshot{
			SchemaVersion: 1, Mode: "bm25",
			RecallAt5: recall, RecallAt10: 0.90,
			NDCGAt5: 0.82, NDCGAt10: 0.88, MRR: 0.92,
			P50LatencyMs: p50, P95LatencyMs: 1.0,
		}
	}
	baseline := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts, CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{mk(baselineRecall, 0.5)},
	}
	current := SnapshotFile{
		SchemaVersion: 1, CreatedAt: ts.Add(time.Hour), CorpusHash: hash, CorpusName: "builtin",
		CorpusSize: 50, QueryCount: 12,
		Snapshots: []BenchSnapshot{mk(currentRecall, currentP50)},
	}
	return CompareSnapshots(baseline, current)
}

func TestWriteComparisonReport_Equal(t *testing.T) {
	comp := comparisonFixture(0.85, 0.85, 0.5)
	var buf bytes.Buffer
	WriteComparisonReport(&buf, comp)
	out := buf.String()

	for _, want := range []string{
		"=== Benchmark Comparison ===",
		"Baseline: 2026-01-01T00:00:00Z (corpus: builtin, hash: aabbccddeeff)",
		"Current:  2026-01-01T01:00:00Z (corpus: builtin, hash: aabbccddeeff)",
		"ALL METRICS PASSED",
		"[bm25]",
		"Recall@5",
		"Summary: All metrics passed",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("expected %q in report, got:\n%s", want, out)
		}
	}
}

func TestWriteComparisonReport_Regressed(t *testing.T) {
	comp := comparisonFixture(0.85, 0.70, 0.5)
	var buf bytes.Buffer
	WriteComparisonReport(&buf, comp)
	out := buf.String()

	for _, want := range []string{
		"REGRESSIONS DETECTED",
		"✗ REGRESSION",
		"↓",
		"Summary: REGRESSIONS FOUND",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("expected %q in regressed report, got:\n%s", want, out)
		}
	}
	if strings.Contains(out, "ALL METRICS PASSED") {
		t.Error("regressed report must not claim all metrics passed")
	}
}

func TestWriteComparisonReport_Improved(t *testing.T) {
	comp := comparisonFixture(0.85, 0.92, 0.3)
	var buf bytes.Buffer
	WriteComparisonReport(&buf, comp)
	out := buf.String()

	for _, want := range []string{
		"ALL METRICS PASSED",
		"↑",
		"Summary: All metrics passed",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("expected %q in improved report, got:\n%s", want, out)
		}
	}
	if strings.Contains(out, "REGRESSION") {
		t.Error("improved report must not mention regressions")
	}
}

func TestWriteComparisonReport_NoComparableMetrics(t *testing.T) {
	ts := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	report := ComparisonReport{
		Baseline: SnapshotFile{SchemaVersion: 1, CreatedAt: ts, CorpusHash: "abc", CorpusName: "builtin"},
		Current:  SnapshotFile{SchemaVersion: 1, CreatedAt: ts, CorpusHash: "abc", CorpusName: "builtin"},
		Passed:   true,
	}
	var buf bytes.Buffer
	WriteComparisonReport(&buf, report)
	out := buf.String()

	if n := strings.Count(out, "(no comparable metrics)"); n != 3 {
		t.Errorf("expected 3 '(no comparable metrics)' placeholders (one per mode), got %d", n)
	}
}

func TestShortHash(t *testing.T) {
	long := "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
	if got := shortHash(long); got != long[:12] {
		t.Errorf("expected first 12 chars, got %q", got)
	}
	exact := "0123456789ab"
	if got := shortHash(exact); got != exact {
		t.Errorf("expected 12-char hash unchanged, got %q", got)
	}
	short := "abc"
	if got := shortHash(short); got != short {
		t.Errorf("expected short hash unchanged, got %q", got)
	}
	if got := shortHash(""); got != "" {
		t.Errorf("expected empty hash unchanged, got %q", got)
	}
}
