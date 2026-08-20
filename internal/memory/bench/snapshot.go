package bench

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// SnapshotsDir is the directory where benchmark baseline snapshots are stored.
// Relative to the working directory when the benchmark is executed.
const SnapshotsDir = ".bench-snapshots"

// BenchSnapshot holds benchmark metrics for a single retrieval mode in a snapshot.
type BenchSnapshot struct {
	SchemaVersion int       `json:"schema_version"`
	Mode          string    `json:"mode"` // bm25, vector, hybrid
	RecallAt5     float64   `json:"recall_at_5"`
	RecallAt10    float64   `json:"recall_at_10"`
	NDCGAt5       float64   `json:"ndcg_at_5"`
	NDCGAt10      float64   `json:"ndcg_at_10"`
	MRR           float64   `json:"mrr"`
	P50LatencyMs  float64   `json:"p50_latency_ms"`
	P95LatencyMs  float64   `json:"p95_latency_ms"`
	Timestamp     time.Time `json:"timestamp"`
	CorpusHash    string    `json:"corpus_hash"`
	CorpusName    string    `json:"corpus_name,omitempty"`
	// Samples carries the per-query values behind the point estimates
	// (issue #490); absent for snapshots written before the stats landed.
	Samples QuerySamples `json:"samples,omitempty"`
}

// SnapshotFile is the complete set of mode snapshots from one benchmark run.
type SnapshotFile struct {
	SchemaVersion int             `json:"schema_version"`
	CreatedAt     time.Time       `json:"created_at"`
	CorpusHash    string          `json:"corpus_hash"`
	CorpusName    string          `json:"corpus_name"`
	CorpusSize    int             `json:"corpus_size"`
	QueryCount    int             `json:"query_count"`
	Snapshots     []BenchSnapshot `json:"snapshots"`
}

// ComputeCorpusHash computes a deterministic SHA-256 hash of a corpus.
// This allows detecting when the corpus changes between benchmark runs.
// The hash is based on memory IDs, scopes, content, and query relevance IDs.
func ComputeCorpusHash(corpus *Corpus) string {
	var b strings.Builder
	// Sort memories by ID for canonical ordering
	memIDs := make([]string, 0, len(corpus.Memories))
	memByID := make(map[string]FixtureMemory, len(corpus.Memories))
	for _, m := range corpus.Memories {
		memIDs = append(memIDs, m.ID)
		memByID[m.ID] = m
	}
	sort.Strings(memIDs)
	for _, id := range memIDs {
		m := memByID[id]
		fmt.Fprintf(&b, "%s|%s|%s\n", m.ID, m.Scope, m.Content)
	}

	// Sort queries by query text for canonical ordering
	qTexts := make([]string, 0, len(corpus.Queries))
	byQText := make(map[string]GroundTruth, len(corpus.Queries))
	for _, q := range corpus.Queries {
		qTexts = append(qTexts, q.Query)
		byQText[q.Query] = q
	}
	sort.Strings(qTexts)
	for _, qt := range qTexts {
		q := byQText[qt]
		sorted := make([]string, len(q.RelevantIDs))
		copy(sorted, q.RelevantIDs)
		sort.Strings(sorted)
		fmt.Fprintf(&b, "%s|%s\n", q.Query, strings.Join(sorted, ","))
	}

	hash := sha256.Sum256([]byte(b.String()))
	return fmt.Sprintf("%x", hash[:])
}

// SnapshotFileName returns the file name for a snapshot given corpus name and hash.
func SnapshotFileName(corpusName, corpusHash string) string {
	short := corpusHash
	if len(short) > 12 {
		short = short[:12]
	}
	return fmt.Sprintf("bench-snapshot-%s-%s.json", corpusName, short)
}

// SnapshotFilePath returns the full path to a snapshot file within snapDir.
func SnapshotFilePath(snapDir, corpusName, corpusHash string) string {
	return filepath.Join(snapDir, SnapshotFileName(corpusName, corpusHash))
}

// ToSnapshotFile converts a Report into the snapshot file format.
func (r *Report) ToSnapshotFile(corpusName, corpusHash string) SnapshotFile {
	sf := SnapshotFile{
		SchemaVersion: 1,
		CreatedAt:     r.Timestamp,
		CorpusHash:    corpusHash,
		CorpusName:    corpusName,
		CorpusSize:    r.CorpusSize,
		QueryCount:    r.QueryCount,
	}
	for _, m := range []RetrievalMetrics{r.BM25, r.Vector, r.Hybrid} {
		sf.Snapshots = append(sf.Snapshots, BenchSnapshot{
			SchemaVersion: 1,
			Mode:          m.Mode,
			RecallAt5:     m.RecallAt5,
			RecallAt10:    m.RecallAt10,
			NDCGAt5:       m.NDCGAt5,
			NDCGAt10:      m.NDCGAt10,
			MRR:           m.MRR,
			P50LatencyMs:  m.P50LatencyMs,
			P95LatencyMs:  m.P95LatencyMs,
			Timestamp:     r.Timestamp,
			CorpusHash:    corpusHash,
			CorpusName:    corpusName,
			Samples:       m.Samples,
		})
	}
	return sf
}

// SaveSnapshot writes a SnapshotFile to disk in snapDir.
// Returns the full path to the saved file.
func SaveSnapshot(snapDir string, snap SnapshotFile) (string, error) {
	if err := os.MkdirAll(snapDir, 0755); err != nil {
		return "", fmt.Errorf("create snapshots dir: %w", err)
	}
	path := SnapshotFilePath(snapDir, snap.CorpusName, snap.CorpusHash)
	f, err := os.Create(path)
	if err != nil {
		return "", fmt.Errorf("create snapshot file: %w", err)
	}
	defer func() { _ = f.Close() }()

	enc := json.NewEncoder(f)
	enc.SetIndent("", "  ")
	if err := enc.Encode(snap); err != nil {
		return "", fmt.Errorf("encode snapshot: %w", err)
	}
	return path, nil
}

// LoadSnapshot reads a SnapshotFile from disk.
func LoadSnapshot(path string) (SnapshotFile, error) {
	var snap SnapshotFile
	f, err := os.Open(path)
	if err != nil {
		return snap, fmt.Errorf("open snapshot: %w", err)
	}
	defer func() { _ = f.Close() }()

	if err := json.NewDecoder(f).Decode(&snap); err != nil {
		return snap, fmt.Errorf("decode snapshot: %w", err)
	}
	return snap, nil
}

// FindBaselineSnapshot looks for an existing snapshot matching the corpus hash.
// Returns zero-value SnapshotFile and empty path when no baseline exists.
func FindBaselineSnapshot(snapDir, corpusName, corpusHash string) (SnapshotFile, string, error) {
	path := SnapshotFilePath(snapDir, corpusName, corpusHash)
	snap, err := LoadSnapshot(path)
	if err != nil {
		// Check the underlying error directly via os.Stat to handle
		// wrapped errors from different Go versions consistently.
		if _, statErr := os.Stat(path); os.IsNotExist(statErr) {
			return SnapshotFile{}, "", nil
		}
		return SnapshotFile{}, "", err
	}
	return snap, path, nil
}

// Delta represents the change in a single metric between baseline and current results.
type Delta struct {
	Metric   string  `json:"metric"`
	Mode     string  `json:"mode"`
	Baseline float64 `json:"baseline"`
	Current  float64 `json:"current"`
	Delta    float64 `json:"delta"` // current - baseline
	Better   bool    `json:"better"`
	// Inferential statistics (issue #490), present when both snapshots
	// carry per-query samples. A metric counts as a regression only when
	// the current 95% CI excludes the baseline value in the worse
	// direction; p is the two-sided Mann-Whitney U p value, corrected
	// across metrics via Holm-Bonferroni.
	CI95Lo      float64 `json:"ci95_lo,omitempty"`
	CI95Hi      float64 `json:"ci95_hi,omitempty"`
	MannWhitney float64 `json:"mann_whitney_p,omitempty"`
	CliffsDelta float64 `json:"cliffs_delta,omitempty"`
}

// ComparisonReport holds the full comparison between baseline and current snapshots.
type ComparisonReport struct {
	Baseline SnapshotFile `json:"baseline"`
	Current  SnapshotFile `json:"current"`
	Deltas   []Delta      `json:"deltas"`
	Passed   bool         `json:"passed"`
}

// samplePicker selects the per-query samples for one metric from a
// snapshot's QuerySamples.
type samplePicker func(s QuerySamples) []float64

var metricPickers = []struct {
	name           string
	higherIsBetter bool
	pick           samplePicker
}{
	{"Recall@5", true, func(s QuerySamples) []float64 { return s.RecallAt5 }},
	{"Recall@10", true, func(s QuerySamples) []float64 { return s.RecallAt10 }},
	{"NDCG@5", true, func(s QuerySamples) []float64 { return s.NDCGAt5 }},
	{"NDCG@10", true, func(s QuerySamples) []float64 { return s.NDCGAt10 }},
	{"MRR", true, func(s QuerySamples) []float64 { return s.MRR }},
	{"P50 Latency (ms)", false, nil},
	{"P95 Latency (ms)", false, nil},
}

// CompareSnapshots compares current results against a baseline and returns
// a ComparisonReport with per-metric deltas. A metric is "better" when it
// improves (higher recall/NDCG/MRR, lower latency). When both snapshots
// carry per-query samples for a quality metric, the regression verdict
// comes from a seeded 95% bootstrap CI over the current samples: the
// metric counts as a regression only when the CI excludes the baseline
// value in the worse direction. Metrics without samples fall back to the
// plain point-estimate delta so legacy snapshots keep working.
func CompareSnapshots(baseline, current SnapshotFile) ComparisonReport {
	report := ComparisonReport{
		Baseline: baseline,
		Current:  current,
	}

	baselineByMode := make(map[string]BenchSnapshot)
	for _, s := range baseline.Snapshots {
		baselineByMode[s.Mode] = s
	}

	type rawMetric struct {
		mode           string
		name           string
		baseline       float64
		current        float64
		higherIsBetter bool
		baseSamples    []float64
		curSamples     []float64
	}
	var raw []rawMetric

	for _, cur := range current.Snapshots {
		base, ok := baselineByMode[cur.Mode]
		if !ok {
			// Mode not in baseline — skip (shouldn't happen with standard modes)
			continue
		}

		type metricDef struct {
			Name           string
			Baseline       float64
			Current        float64
			HigherIsBetter bool
			Pick           samplePicker
		}
		metrics := []metricDef{
			{"Recall@5", base.RecallAt5, cur.RecallAt5, true, metricPickers[0].pick},
			{"Recall@10", base.RecallAt10, cur.RecallAt10, true, metricPickers[1].pick},
			{"NDCG@5", base.NDCGAt5, cur.NDCGAt5, true, metricPickers[2].pick},
			{"NDCG@10", base.NDCGAt10, cur.NDCGAt10, true, metricPickers[3].pick},
			{"MRR", base.MRR, cur.MRR, true, metricPickers[4].pick},
			{"P50 Latency (ms)", base.P50LatencyMs, cur.P50LatencyMs, false, nil},
			{"P95 Latency (ms)", base.P95LatencyMs, cur.P95LatencyMs, false, nil},
		}

		for _, m := range metrics {
			raw = append(raw, rawMetric{
				mode:           cur.Mode,
				name:           m.Name,
				baseline:       m.Baseline,
				current:        m.Current,
				higherIsBetter: m.HigherIsBetter,
				baseSamples:    base.Samples.pickOrEmpty(m.Pick),
				curSamples:     cur.Samples.pickOrEmpty(m.Pick),
			})
		}
	}

	// Holm-Bonferroni correction across all Mann-Whitney tests.
	pvals := make([]float64, len(raw))
	for i, m := range raw {
		pvals[i] = 1
		if len(m.baseSamples) >= 2 && len(m.curSamples) >= 2 {
			_, _, p, err := MannWhitneyU(m.curSamples, m.baseSamples)
			if err == nil {
				pvals[i] = p
			}
		}
	}
	adjusted := HolmBonferroni(pvals)

	regression := false
	for i, m := range raw {
		deltaVal := m.current - m.baseline
		var better bool
		if m.higherIsBetter {
			better = deltaVal >= 0
		} else {
			better = deltaVal <= 0 // latency decreased = better
		}

		d := Delta{
			Metric:      m.name,
			Mode:        m.mode,
			Baseline:    m.baseline,
			Current:     m.current,
			Delta:       deltaVal,
			Better:      better,
			MannWhitney: adjusted[i],
		}
		if len(m.baseSamples) >= 2 && len(m.curSamples) >= 2 {
			// Quality metrics: verdict from the seeded bootstrap CI over
			// current per-query samples. Deterministic for a fixed corpus.
			// The CI verdict is authoritative: a metric is a regression
			// only when the CI excludes the baseline in the worse
			// direction; a dip whose CI still reaches the baseline is
			// indistinguishable from run noise and counts as passed.
			seed := int64(0x5eed) + int64(i)*2654435761
			lo, hi, err := SeededBootstrapCI(m.curSamples, seed, 2000, 0.05)
			if err == nil {
				d.CI95Lo = lo
				d.CI95Hi = hi
				d.CliffsDelta = CliffsDelta(m.curSamples, m.baseSamples)
				if m.higherIsBetter {
					better = hi >= m.baseline
				} else {
					better = lo <= m.baseline
				}
				d.Better = better
			}
		}
		if !d.Better {
			regression = true
		}
		report.Deltas = append(report.Deltas, d)
	}

	report.Passed = !regression
	return report
}

// pickOrEmpty resolves a metric's per-query samples from a snapshot,
// returning nil when the picker is nil or the samples are absent.
func (s QuerySamples) pickOrEmpty(pick samplePicker) []float64 {
	if pick == nil {
		return nil
	}
	return pick(s)
}

// WriteComparisonReport writes the comparison report to w in human-readable format.
func WriteComparisonReport(w io.Writer, report ComparisonReport) {
	fmt.Fprintf(w, "=== Benchmark Comparison ===\n\n")
	fmt.Fprintf(w, "Baseline: %s (corpus: %s, hash: %s)\n",
		report.Baseline.CreatedAt.Format(time.RFC3339),
		report.Baseline.CorpusName,
		shortHash(report.Baseline.CorpusHash))
	fmt.Fprintf(w, "Current:  %s (corpus: %s, hash: %s)\n\n",
		report.Current.CreatedAt.Format(time.RFC3339),
		report.Current.CorpusName,
		shortHash(report.Current.CorpusHash))

	if report.Passed {
		fmt.Fprintf(w, "✓ ALL METRICS PASSED — no regressions detected\n\n")
	} else {
		fmt.Fprintf(w, "✗ REGRESSIONS DETECTED\n\n")
	}

	modes := []string{"bm25", "vector", "hybrid"}
	for _, mode := range modes {
		fmt.Fprintf(w, "  [%s]\n", mode)
		hasOutput := false
		for _, d := range report.Deltas {
			if d.Mode != mode {
				continue
			}
			hasOutput = true
			arrow := "↑"
			if !d.Better {
				arrow = "↓"
			}
			status := "✓"
			if !d.Better {
				status = "✗ REGRESSION"
			}
			fmt.Fprintf(w, "    %-20s  baseline=%.4f  current=%.4f  Δ%+9.4f  %s  %s\n",
				d.Metric, d.Baseline, d.Current, d.Delta, arrow, status)
			if d.CI95Hi > 0 || d.CI95Lo > 0 {
				fmt.Fprintf(w, "       95%% CI [%.4f, %.4f]  Mann-Whitney p=%.4f  Cliff's Δ=%.3f\n",
					d.CI95Lo, d.CI95Hi, d.MannWhitney, d.CliffsDelta)
			}
		}
		if !hasOutput {
			fmt.Fprintf(w, "    (no comparable metrics)\n")
		}
		fmt.Fprintf(w, "\n")
	}

	if !report.Passed {
		fmt.Fprintf(w, "Summary: REGRESSIONS FOUND — review the deltas above.\n")
	} else {
		fmt.Fprintf(w, "Summary: All metrics passed — performance is stable.\n")
	}
}

// shortHash returns the first 12 characters of a hex hash.
func shortHash(h string) string {
	if len(h) > 12 {
		return h[:12]
	}
	return h
}
