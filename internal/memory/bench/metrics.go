package bench

import (
	"math"
	"sort"
	"time"
)

// RetrievalMetrics holds the computed quality metrics for a single retrieval mode.
type RetrievalMetrics struct {
	Mode          string  `json:"mode"` // "bm25", "vector", "hybrid"
	RecallAt5     float64 `json:"recall_at_5"`
	RecallAt10    float64 `json:"recall_at_10"`
	NDCGAt5       float64 `json:"ndcg_at_5"`
	NDCGAt10      float64 `json:"ndcg_at_10"`
	MRR           float64 `json:"mrr"`
	MeanLatencyMs float64 `json:"mean_latency_ms"`
	P50LatencyMs  float64 `json:"p50_latency_ms"`
	P95LatencyMs  float64 `json:"p95_latency_ms"`
	QueryCount    int     `json:"query_count"`
	ValidFraction float64 `json:"valid_fraction,omitempty"` // temporal validity slice
	ScopeFraction float64 `json:"scope_fraction,omitempty"` // scope isolation slice
	// Samples holds the per-query metric values (one entry per answerable
	// query, pooled across repeat runs) that back the aggregate metrics.
	// They enable bootstrap CIs, Mann-Whitney U and Cliff's delta in
	// CompareSnapshots (issue #490). Omitted for legacy snapshots.
	Samples QuerySamples `json:"samples,omitempty"`
}

// QuerySamples holds the per-query values behind each aggregate metric.
type QuerySamples struct {
	RecallAt5  []float64 `json:"recall_at_5,omitempty"`
	RecallAt10 []float64 `json:"recall_at_10,omitempty"`
	NDCGAt5    []float64 `json:"ndcg_at_5,omitempty"`
	NDCGAt10   []float64 `json:"ndcg_at_10,omitempty"`
	MRR        []float64 `json:"mrr,omitempty"`
}

// Empty reports whether no query samples were recorded.
func (s QuerySamples) Empty() bool {
	return len(s.RecallAt5) == 0 && len(s.RecallAt10) == 0 && len(s.NDCGAt5) == 0 && len(s.NDCGAt10) == 0 && len(s.MRR) == 0
}

// RecallAtK computes the fraction of relevant documents that appear in the top-k results.
// relevant is the set of ground-truth relevant IDs; retrieved is the ordered list of result IDs.
func RecallAtK(retrieved []string, relevant map[string]bool, k int) float64 {
	if len(relevant) == 0 || k == 0 {
		return 0
	}
	if k > len(retrieved) {
		k = len(retrieved)
	}
	found := 0
	for i := 0; i < k; i++ {
		if relevant[retrieved[i]] {
			found++
		}
	}
	return float64(found) / float64(len(relevant))
}

// NDCGAtK computes Normalized Discounted Cumulative Gain at rank k.
// relevanceScores maps document IDs to binary relevance (1 = relevant, 0 = not).
// retrieved is the ordered list of result IDs from the retrieval system.
func NDCGAtK(retrieved []string, relevanceScores map[string]int, k int) float64 {
	if len(relevanceScores) == 0 || k == 0 {
		return 0
	}
	if k > len(retrieved) {
		k = len(retrieved)
	}

	// Compute DCG
	dcg := 0.0
	for i := 0; i < k; i++ {
		if rel, ok := relevanceScores[retrieved[i]]; ok && rel > 0 {
			dcg += float64(rel) / math.Log2(float64(i+2)) // i+2 because log2(1) = 0
		}
	}

	// Compute ideal DCG
	idealRels := make([]int, 0, len(relevanceScores))
	for _, rel := range relevanceScores {
		if rel > 0 {
			idealRels = append(idealRels, rel)
		}
	}
	sort.Sort(sort.Reverse(sort.IntSlice(idealRels)))

	idcg := 0.0
	for i := 0; i < k && i < len(idealRels); i++ {
		idcg += float64(idealRels[i]) / math.Log2(float64(i+2))
	}

	if idcg == 0 {
		return 0
	}
	return dcg / idcg
}

// MRR computes Mean Reciprocal Rank: the average of 1/rank for each query
// where rank is the position of the first relevant result.
func MRR(retrieved []string, relevant map[string]bool) float64 {
	if len(relevant) == 0 {
		return 0
	}
	for i, id := range retrieved {
		if relevant[id] {
			return 1.0 / float64(i+1)
		}
	}
	return 0
}

// AbstentionReport measures how often the system correctly abstains on
// unanswerable queries and answers on answerable ones at a score threshold.
type AbstentionReport struct {
	Mode      string  `json:"mode"`
	Threshold float64 `json:"threshold"`
	Correct   int     `json:"correct"`
	Total     int     `json:"total"`
	Accuracy  float64 `json:"accuracy"`
}

// ComputeAbstention scores abstention decisions. topScores maps query index to
// the system's best score for that query (0 when nothing was retrieved); a query
// counts as abstained when its top score is below threshold. A decision is
// correct when an unanswerable query is abstained or an answerable one is not.
func ComputeAbstention(mode string, threshold float64, queries []GroundTruth, topScores map[int]float64) AbstentionReport {
	report := AbstentionReport{Mode: mode, Threshold: threshold, Total: len(queries)}
	for i, gt := range queries {
		abstained := topScores[i] < threshold
		if abstained != gt.Answerable {
			report.Correct++
		}
	}
	if report.Total > 0 {
		report.Accuracy = float64(report.Correct) / float64(report.Total)
	}
	return report
}

// LatencyPercentiles computes P50 and P95 from a slice of durations.
func LatencyPercentiles(durations []time.Duration) (p50, p95 time.Duration) {
	if len(durations) == 0 {
		return 0, 0
	}
	sorted := make([]time.Duration, len(durations))
	copy(sorted, durations)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })

	p50Idx := int(math.Ceil(0.50*float64(len(sorted)))) - 1
	p95Idx := int(math.Ceil(0.95*float64(len(sorted)))) - 1
	if p50Idx < 0 {
		p50Idx = 0
	}
	if p95Idx < 0 {
		p95Idx = 0
	}
	if p50Idx >= len(sorted) {
		p50Idx = len(sorted) - 1
	}
	if p95Idx >= len(sorted) {
		p95Idx = len(sorted) - 1
	}
	return sorted[p50Idx], sorted[p95Idx]
}

// ComputeMetrics computes aggregate retrieval metrics across all queries for a single mode.
// queryResults maps query index to ordered list of retrieved memory IDs.
// groundTruth is the ordered slice of queries with their relevant ID sets.
func ComputeMetrics(mode string, queryResults map[int][]string, groundTruth []GroundTruth, latencies []time.Duration) RetrievalMetrics {
	return ComputeMetricsPooled(mode, []map[int][]string{queryResults}, groundTruth, [][]time.Duration{latencies})
}

// ComputeMetricsPooled computes aggregate retrieval metrics across one or
// more runs of the same queries (issue #490). Per-query metric values are
// pooled across runs into Samples so bootstrap CIs and significance tests
// can be computed against the stored baseline; latency samples are pooled
// the same way.
func ComputeMetricsPooled(mode string, runResults []map[int][]string, groundTruth []GroundTruth, runLatencies [][]time.Duration) RetrievalMetrics {
	var recall5Sum, recall10Sum, ndcg5Sum, ndcg10Sum, mrrSum float64
	validCount := 0
	var samples QuerySamples

	for _, queryResults := range runResults {
		for i, gt := range groundTruth {
			if len(gt.RelevantIDs) == 0 {
				continue // unanswerable abstention queries carry no retrieval ground truth
			}
			retrieved := queryResults[i]
			relevant := make(map[string]bool)
			for _, id := range gt.RelevantIDs {
				relevant[id] = true
			}

			// Binary relevance for NDCG
			relevanceScores := make(map[string]int)
			for _, id := range gt.RelevantIDs {
				relevanceScores[id] = 1
			}

			r5 := RecallAtK(retrieved, relevant, 5)
			r10 := RecallAtK(retrieved, relevant, 10)
			ndcg5 := NDCGAtK(retrieved, relevanceScores, 5)
			ndcg10 := NDCGAtK(retrieved, relevanceScores, 10)
			mrr := MRR(retrieved, relevant)

			recall5Sum += r5
			recall10Sum += r10
			ndcg5Sum += ndcg5
			ndcg10Sum += ndcg10
			mrrSum += mrr
			validCount++

			samples.RecallAt5 = append(samples.RecallAt5, r5)
			samples.RecallAt10 = append(samples.RecallAt10, r10)
			samples.NDCGAt5 = append(samples.NDCGAt5, ndcg5)
			samples.NDCGAt10 = append(samples.NDCGAt10, ndcg10)
			samples.MRR = append(samples.MRR, mrr)
		}
	}

	n := float64(validCount)
	if n == 0 {
		n = 1
	}

	var allLatencies []time.Duration
	for _, run := range runLatencies {
		allLatencies = append(allLatencies, run...)
	}

	var meanLat, p50, p95 float64
	if len(allLatencies) > 0 {
		p50d, p95d := LatencyPercentiles(allLatencies)
		p50 = float64(p50d.Microseconds()) / 1000.0
		p95 = float64(p95d.Microseconds()) / 1000.0
		var total time.Duration
		for _, d := range allLatencies {
			total += d
		}
		meanLat = float64(total.Microseconds()) / float64(len(allLatencies)) / 1000.0
	}

	return RetrievalMetrics{
		Mode:          mode,
		RecallAt5:     recall5Sum / n,
		RecallAt10:    recall10Sum / n,
		NDCGAt5:       ndcg5Sum / n,
		NDCGAt10:      ndcg10Sum / n,
		MRR:           mrrSum / n,
		MeanLatencyMs: meanLat,
		P50LatencyMs:  p50,
		P95LatencyMs:  p95,
		QueryCount:    validCount,
		Samples:       samples,
	}
}
