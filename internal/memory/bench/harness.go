package bench

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

// Options configures a benchmark run.
type Options struct {
	Repetitions      int     // Number of repetitions for latency measurement (default: 10)
	Output           string  // Output format: "text" or "json" (default: "text")
	FixturePath      string  // Path to custom JSON fixture file (optional)
	Dataset          string  // External dataset name for opt-in datasets (optional; alias for Corpus)
	Corpus           string  // Corpus to evaluate: "builtin" (default) or "longmemeval"
	AbstainThreshold float64 // Score threshold for abstention evaluation (default 0)
	// RepeatRuns reruns the whole query evaluation this many times and
	// pools the per-query samples (issue #490). Default 1 = single run.
	RepeatRuns int
	// Seed makes the bootstrap resampling in CI comparisons deterministic.
	Seed int64
}

// Report holds the complete benchmark results.
type Report struct {
	Timestamp       time.Time          `json:"timestamp"`
	CorpusSize      int                `json:"corpus_size"`
	QueryCount      int                `json:"query_count"`
	Repetitions     int                `json:"repetitions"`
	EmbeddingSource string             `json:"embedding_source"`
	BM25            RetrievalMetrics   `json:"bm25"`
	Vector          RetrievalMetrics   `json:"vector"`
	Hybrid          RetrievalMetrics   `json:"hybrid"`
	Temporal        []TemporalReport   `json:"temporal,omitempty"`
	Scope           []ScopeReport      `json:"scope,omitempty"`
	Abstention      []AbstentionReport `json:"abstention,omitempty"`

	// #402: Context-pressure and recovery canary benchmarks
	ContextPressure []ContextPressureReport `json:"context_pressure,omitempty"`
	RecoveryCanary  []RecoveryCanaryReport  `json:"recovery_canary,omitempty"`

	// #490: external baseline arm (skipped unless configured)
	Baseline BaselineArmReport `json:"baseline,omitempty"`

	// #491: aging assessment (hermetic synthetic-store comparison)
	Aging AgingReport `json:"aging,omitempty"`
}

// TemporalReport holds temporal-validity evaluation results for a single mode.
type TemporalReport struct {
	Mode          string  `json:"mode"`
	ValidFraction float64 `json:"valid_fraction"`
	Description   string  `json:"description"`
}

// ScopeReport holds scope-isolation evaluation results for a single mode.
type ScopeReport struct {
	Mode        string `json:"mode"`
	AllInScope  bool   `json:"all_in_scope"`
	Description string `json:"description"`
}

// Run executes the benchmark harness and writes results to w.
// It creates a temporary database, loads the fixture corpus, and measures
// retrieval quality and latency across BM25, vector, and hybrid modes.
func Run(w io.Writer, opts Options) error {
	if opts.Repetitions <= 0 {
		opts.Repetitions = 10
	}
	if opts.Output == "" {
		opts.Output = "text"
	}

	// Load corpus
	corpusName := opts.Corpus
	if corpusName == "" {
		corpusName = opts.Dataset
	}
	var corpus *Corpus
	switch corpusName {
	case "", "builtin":
		corpus = DefaultCorpus()
	case "longmemeval":
		if opts.FixturePath == "" {
			return fmt.Errorf("corpus longmemeval requires a dataset file path (--path)")
		}
		loaded, err := LoadLongMemEval(opts.FixturePath)
		if err != nil {
			return err
		}
		corpus = loaded
	default:
		return fmt.Errorf("unknown corpus %q: must be \"builtin\" or \"longmemeval\"", corpusName)
	}

	// Create temporary database
	tempDir, err := os.MkdirTemp("", "symmemory-bench-*")
	if err != nil {
		return fmt.Errorf("failed to create temp dir: %w", err)
	}
	defer os.RemoveAll(tempDir)

	cfg := config.Defaults()
	cfg.Database.Path = tempDir + "/bench.db"
	database, err := db.Open(cfg)
	if err != nil {
		return fmt.Errorf("failed to open benchmark database: %w", err)
	}
	defer func() { _ = database.Close() }()

	// Generate embeddings and populate corpus
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	// Force hash-fallback mode for determinism
	embeddings.MarkOllamaFailed()

	embeddingSource := "hash-fallback"

	for _, mem := range corpus.Memories {
		vec := embeddings.GenerateVector(mem.Content)
		dbMem := &db.Memory{
			ID:              mem.ID,
			Content:         mem.Content,
			Scope:           mem.Scope,
			Metadata:        mem.Metadata,
			Embedding:       vec.Vector,
			EmbeddingSource: vec.Source,
			Importance:      0.5,
			ValidFrom:       mem.ValidFrom,
			ValidTo:         mem.ValidTo,
		}
		if dbMem.Metadata == nil {
			dbMem.Metadata = map[string]string{}
		}
		if err := database.SaveMemory(dbMem); err != nil {
			return fmt.Errorf("failed to save fixture memory %s: %w", mem.ID, err)
		}
	}

	report := Report{
		Timestamp:       time.Now().UTC(),
		CorpusSize:      len(corpus.Memories),
		QueryCount:      len(corpus.Queries),
		Repetitions:     opts.Repetitions,
		EmbeddingSource: embeddingSource,
	}

	// --- Run BM25 evaluation ---
	bm25Runs, bm25LatencyRuns := runModeRuns(database, corpus, "bm25", opts.Repetitions, opts.RepeatRuns)
	report.BM25 = ComputeMetricsPooled("bm25", bm25Runs, corpus.Queries, bm25LatencyRuns)

	// --- Run Vector evaluation ---
	vectorRuns, vectorLatencyRuns := runModeRuns(database, corpus, "vector", opts.Repetitions, opts.RepeatRuns)
	report.Vector = ComputeMetricsPooled("vector", vectorRuns, corpus.Queries, vectorLatencyRuns)

	// --- Run Hybrid evaluation ---
	hybridRuns, hybridLatencyRuns := runModeRuns(database, corpus, "hybrid", opts.Repetitions, opts.RepeatRuns)
	report.Hybrid = ComputeMetricsPooled("hybrid", hybridRuns, corpus.Queries, hybridLatencyRuns)

	// --- Temporal validity evaluation ---
	report.Temporal = evaluateTemporal(database, corpus, embeddings)

	// --- Scope isolation evaluation ---
	report.Scope = evaluateScope(database, corpus, embeddings)

	// --- Abstention evaluation (only for corpora with unanswerable queries) ---
	if HasUnanswerableQueries(corpus) {
		report.Abstention = evaluateAbstention(database, corpus, embeddings, opts.AbstainThreshold)
	}

	// --- External baseline arm (env-gated, skipped unless configured) ---
	baseline, err := RunExternalBaseline(corpus)
	if err != nil {
		return err // configured but failing: fail loudly, never fake the arm
	}
	report.Baseline = baseline

	// --- Aging assessment (#491): synthetic-store decay comparison ---
	seed := opts.Seed
	if seed == 0 {
		seed = 42
	}
	agingReport, err := RunAgingAssessment(seed)
	if err != nil {
		return err
	}
	report.Aging = agingReport

	// --- Output ---
	switch opts.Output {
	case "json":
		enc := json.NewEncoder(w)
		enc.SetIndent("", "  ")
		return enc.Encode(report)
	default:
		return writeTextReport(w, report)
	}
}

// runModeRuns executes all queries for a mode, repeatRuns times, and
// returns the per-query results and latencies of every run. With
// repeatRuns <= 1 it behaves like runMode with a single run. The
// repetitions parameter stays the per-query latency measurement count.
func runModeRuns(database *db.DB, corpus *Corpus, mode string, repetitions, repeatRuns int) ([]map[int][]string, [][]time.Duration) {
	if repeatRuns <= 0 {
		repeatRuns = 1
	}
	allRuns := make([]map[int][]string, 0, repeatRuns)
	allLatencyRuns := make([][]time.Duration, 0, repeatRuns)

	for run := 0; run < repeatRuns; run++ {
		queryResults := make(map[int][]string)
		var allLatencies []time.Duration

		for i, gt := range corpus.Queries {
			var bestResults []string
			var latencies []time.Duration

			for rep := 0; rep < repetitions; rep++ {
				start := time.Now()
				var ids []string

				switch mode {
				case "bm25":
					results, err := database.SearchMemoriesBM25(gt.Query, "", 10)
					if err == nil {
						for _, r := range results {
							ids = append(ids, r.Memory.ID)
						}
					}
				case "vector":
					vec := extractor.GenerateLocalHashVector(gt.Query, extractor.DefaultDimensions)
					results, err := database.SearchMemories(vec, "hash-fallback", "", 10)
					if err == nil {
						for _, r := range results {
							ids = append(ids, r.Memory.ID)
						}
					}
				case "hybrid":
					vec := extractor.GenerateLocalHashVector(gt.Query, extractor.DefaultDimensions)
					results, err := database.HybridSearch(vec, "hash-fallback", gt.Query, "", 10, "", db.TrustFilter{}, db.PolicyFilter{}, 0.7, 0.3)
					if err == nil {
						for _, r := range results {
							ids = append(ids, r.Memory.ID)
						}
					}
				}

				elapsed := time.Since(start)
				latencies = append(latencies, elapsed)

				// Use the result from the first repetition (deterministic corpus)
				if rep == 0 {
					bestResults = ids
				}
			}

			queryResults[i] = bestResults
			allLatencies = append(allLatencies, latencies...)
		}

		allRuns = append(allRuns, queryResults)
		allLatencyRuns = append(allLatencyRuns, allLatencies)
	}

	return allRuns, allLatencyRuns
}

// evaluateTemporal checks what fraction of top-5 results for temporal queries
// are currently valid (ValidTo is nil or in the future).
func evaluateTemporal(database *db.DB, corpus *Corpus, embeddings *extractor.EmbeddingsGenerator) []TemporalReport {
	var reports []TemporalReport

	for _, ts := range corpus.TemporalSlices {
		for _, mode := range []string{"bm25", "vector", "hybrid"} {
			var ids []string

			switch mode {
			case "bm25":
				results, err := database.SearchMemoriesBM25(ts.Query, "", 5)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			case "vector":
				vec := embeddings.GenerateVector(ts.Query)
				results, err := database.SearchMemories(vec.Vector, vec.Source, "", 5)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			case "hybrid":
				vec := embeddings.GenerateVector(ts.Query)
				results, err := database.HybridSearch(vec.Vector, vec.Source, ts.Query, "", 5, "", db.TrustFilter{}, db.PolicyFilter{}, 0.7, 0.3)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			}

			validSet := make(map[string]bool)
			for _, id := range ts.CurrentlyValid {
				validSet[id] = true
			}

			validCount := 0
			for _, id := range ids {
				if validSet[id] {
					validCount++
				}
			}

			fraction := 0.0
			if len(ids) > 0 {
				fraction = float64(validCount) / float64(len(ids))
			}

			reports = append(reports, TemporalReport{
				Mode:          mode,
				ValidFraction: fraction,
				Description:   fmt.Sprintf("%s [mode=%s]: %d/%d top-5 results are currently valid", ts.Description, mode, validCount, len(ids)),
			})
		}
	}

	return reports
}

// evaluateScope checks that scope-constrained queries only return memories
// from the expected scope.
func evaluateScope(database *db.DB, corpus *Corpus, embeddings *extractor.EmbeddingsGenerator) []ScopeReport {
	var reports []ScopeReport

	for _, ss := range corpus.ScopeSlices {
		for _, mode := range []string{"bm25", "vector", "hybrid"} {
			var ids []string

			switch mode {
			case "bm25":
				results, err := database.SearchMemoriesBM25(ss.Query, ss.Scope, 10)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			case "vector":
				vec := embeddings.GenerateVector(ss.Query)
				results, err := database.SearchMemories(vec.Vector, vec.Source, ss.Scope, 10)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			case "hybrid":
				vec := embeddings.GenerateVector(ss.Query)
				results, err := database.HybridSearch(vec.Vector, vec.Source, ss.Query, ss.Scope, 10, "", db.TrustFilter{}, db.PolicyFilter{}, 0.7, 0.3)
				if err == nil {
					for _, r := range results {
						ids = append(ids, r.Memory.ID)
					}
				}
			}

			// Verify all returned IDs have the correct scope
			expectedSet := make(map[string]bool)
			for _, id := range ss.ExpectedIDs {
				expectedSet[id] = true
			}

			allInScope := true
			for _, id := range ids {
				// Check that the memory exists and has the right scope
				mem, err := database.GetMemory(id)
				if err != nil || mem == nil || mem.Scope != ss.Scope {
					allInScope = false
					break
				}
			}

			_ = expectedSet // used for reporting context, not strict check

			reports = append(reports, ScopeReport{
				Mode:        mode,
				AllInScope:  allInScope,
				Description: fmt.Sprintf("%s [mode=%s]: all %d results in scope=%s: %v", ss.Description, mode, len(ids), ss.Scope, allInScope),
			})
		}
	}

	return reports
}

// evaluateAbstention runs every query in hybrid mode and measures how often the
// system abstains (top fused score below threshold) exactly when it should.
func evaluateAbstention(database *db.DB, corpus *Corpus, embeddings *extractor.EmbeddingsGenerator, threshold float64) []AbstentionReport {
	topScores := make(map[int]float64, len(corpus.Queries))
	for i, gt := range corpus.Queries {
		vec := embeddings.GenerateVector(gt.Query)
		results, err := database.HybridSearch(vec.Vector, vec.Source, gt.Query, "", 10, "", db.TrustFilter{}, db.PolicyFilter{}, 0.7, 0.3)
		if err == nil && len(results) > 0 {
			topScores[i] = results[0].FusedScore
		}
	}
	return []AbstentionReport{ComputeAbstention("hybrid", threshold, corpus.Queries, topScores)}
}

// writeTextReport writes a human-readable benchmark report to w.
func writeTextReport(w io.Writer, report Report) error {
	fmt.Fprintf(w, "=== Symaira Memory Retrieval Benchmark ===\n\n")
	fmt.Fprintf(w, "Corpus:     %d memories\n", report.CorpusSize)
	fmt.Fprintf(w, "Queries:    %d evaluation queries\n", report.QueryCount)
	fmt.Fprintf(w, "Reps:       %d per query for latency\n", report.Repetitions)
	fmt.Fprintf(w, "Embeddings: %s\n\n", report.EmbeddingSource)

	fmt.Fprintf(w, "                    Recall@5  Recall@10  NDCG@5   NDCG@10  MRR      P50(ms)  P95(ms)\n")
	fmt.Fprintf(w, "─────────────────  ────────  ────────   ───────  ───────  ───────  ───────  ───────\n")

	for _, m := range []RetrievalMetrics{report.BM25, report.Vector, report.Hybrid} {
		fmt.Fprintf(w, "  %-16s %7.3f   %7.3f    %6.3f   %6.3f   %6.3f   %7.2f  %7.2f\n",
			m.Mode, m.RecallAt5, m.RecallAt10, m.NDCGAt5, m.NDCGAt10, m.MRR,
			m.P50LatencyMs, m.P95LatencyMs)
	}

	fmt.Fprintf(w, "\n--- Temporal Validity ---\n")
	for _, t := range report.Temporal {
		fmt.Fprintf(w, "  %s\n", t.Description)
	}

	fmt.Fprintf(w, "\n--- Scope Isolation ---\n")
	for _, s := range report.Scope {
		fmt.Fprintf(w, "  %s\n", s.Description)
	}

	if len(report.Abstention) > 0 {
		fmt.Fprintf(w, "\n--- Abstention ---\n")
		for _, a := range report.Abstention {
			fmt.Fprintf(w, "  [mode=%s] threshold=%.3f: %d/%d correct (accuracy %.3f)\n",
				a.Mode, a.Threshold, a.Correct, a.Total, a.Accuracy)
		}
	}

	if report.Aging.Queries > 0 {
		fmt.Fprintf(w, "\n--- Aging Assessment (#491) ---\n")
		fmt.Fprintf(w, "  store=%d facts (%d%% stale), %d queries: MRR %.3f -> %.3f (delta %+.3f), stale-in-top5 %.2f -> %.2f (delta %+.2f)\n",
			report.Aging.StoreSize, int(report.Aging.StalePct), report.Aging.Queries,
			report.Aging.MRRNoDecay, report.Aging.MRRDecay, report.Aging.MRRDelta,
			report.Aging.StaleCrowdNoDecay, report.Aging.StaleCrowdDecay, report.Aging.StaleCrowdDelta)
		fmt.Fprintf(w, "  %s\n", report.Aging.Description)
	}

	fmt.Fprintf(w, "\n=== Benchmark Complete ===\n")
	return nil
}
