package db

import (
	"math"
	"sync"
	"time"
)

// RetrievalStats is a thread-safe accumulator for live retrieval health
// statistics. It records total queries, zero-result queries, and score
// min/max/avg across all search paths.
type RetrievalStats struct {
	mu                sync.Mutex
	totalQueries      int64
	zeroResultQueries int64
	scoreMin          float64
	scoreMax          float64
	scoreSum          float64
	scoreCount        int64
	totalLatencyNs    int64
}

// Record records a single retrieval query result.
// numResults is the number of results returned (0 for zero-result queries).
// topScore is the highest score among results; when numResults is 0, pass 0.
// latency is the wall-clock duration of the query.
func (rs *RetrievalStats) Record(numResults int, topScore float64, latency time.Duration) {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	rs.totalQueries++
	rs.totalLatencyNs += latency.Nanoseconds()

	if numResults == 0 {
		rs.zeroResultQueries++
		return
	}

	if rs.scoreCount == 0 {
		rs.scoreMin = topScore
		rs.scoreMax = topScore
	} else {
		rs.scoreMin = math.Min(rs.scoreMin, topScore)
		rs.scoreMax = math.Max(rs.scoreMax, topScore)
	}
	rs.scoreSum += topScore
	rs.scoreCount++
}

// Snapshot returns a consistent point-in-time snapshot of the stats.
func (rs *RetrievalStats) Snapshot() RetrievalStatsSnapshot {
	rs.mu.Lock()
	defer rs.mu.Unlock()

	var avgScore float64
	if rs.scoreCount > 0 {
		avgScore = rs.scoreSum / float64(rs.scoreCount)
	}

	var avgLatencyMs float64
	if rs.totalQueries > 0 {
		avgLatencyMs = float64(rs.totalLatencyNs) / float64(rs.totalQueries) / 1e6
	}

	return RetrievalStatsSnapshot{
		TotalQueries:      rs.totalQueries,
		ZeroResultQueries: rs.zeroResultQueries,
		ScoreMin:          rs.scoreMin,
		ScoreMax:          rs.scoreMax,
		ScoreAvg:          avgScore,
		AvgLatencyMs:      avgLatencyMs,
	}
}

// Reset clears all accumulated stats.
func (rs *RetrievalStats) Reset() {
	rs.mu.Lock()
	defer rs.mu.Unlock()
	rs.totalQueries = 0
	rs.zeroResultQueries = 0
	rs.scoreMin = 0
	rs.scoreMax = 0
	rs.scoreSum = 0
	rs.scoreCount = 0
	rs.totalLatencyNs = 0
}

// RetrievalStatsSnapshot is a point-in-time snapshot of retrieval stats.
type RetrievalStatsSnapshot struct {
	TotalQueries      int64   `json:"total_queries"`
	ZeroResultQueries int64   `json:"zero_result_queries"`
	ScoreMin          float64 `json:"score_min"`
	ScoreMax          float64 `json:"score_max"`
	ScoreAvg          float64 `json:"score_avg"`
	AvgLatencyMs      float64 `json:"avg_latency_ms"`
}
