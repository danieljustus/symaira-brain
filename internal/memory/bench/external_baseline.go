package bench

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"time"
)

// External baseline arm (issue #490b): one env-gated, dependency-free
// baseline retriever so "our retrieval is good" is tested against
// something other than ourselves.
//
// The arm is skipped entirely (CI default) unless SYMMEMORY_BENCH_BASELINE_URL
// is set. When set, the endpoint must accept
//
//	{"queries":[{"id":0,"query":"..."}, ...]}
//
// and respond with
//
//	{"results":[{"query_id":0,"ids":["mem-1","mem-2",...]}, ...]}
//
// returning top-k memory ids per query. Recall@5 is then computed against
// the same ground truth as the built-in modes. A configured endpoint that
// is unreachable or malformed fails loudly with an actionable error —
// never silently degrades.

// BaselineURLEnv is the environment variable that enables the arm.
const BaselineURLEnv = "SYMMEMORY_BENCH_BASELINE_URL"

// baselineTimeout bounds a single arm call so a dead endpoint cannot hang
// the benchmark.
const baselineTimeout = 30 * time.Second

// BaselineArmReport carries the arm's outcome. Skipped=true means the arm
// was not configured; Error is set only when configured and failed.
type BaselineArmReport struct {
	Configured bool      `json:"configured"`
	Skipped    bool      `json:"skipped,omitempty"`
	RecallAt5  float64   `json:"recall_at_5,omitempty"`
	QueryCount int       `json:"query_count,omitempty"`
	Error      string    `json:"error,omitempty"`
	PerQuery   []float64 `json:"per_query_recall_at_5,omitempty"`
}

// baselineRequest is the wire payload sent to the baseline endpoint.
type baselineRequest struct {
	Queries []baselineQuery `json:"queries"`
}

type baselineQuery struct {
	ID    int    `json:"id"`
	Query string `json:"query"`
}

// baselineResponse is the wire payload expected back.
type baselineResponse struct {
	Results []baselineResult `json:"results"`
}

type baselineResult struct {
	QueryID int      `json:"query_id"`
	IDs     []string `json:"ids"`
}

// RunExternalBaseline evaluates the corpus queries against the configured
// external baseline endpoint and returns Recall@5 per the corpus ground
// truth. Unconfigured: skipped, nil error. Configured but failing: an
// actionable error.
func RunExternalBaseline(corpus *Corpus) (BaselineArmReport, error) {
	url := os.Getenv(BaselineURLEnv)
	if url == "" {
		return BaselineArmReport{Configured: false, Skipped: true}, nil
	}

	req := baselineRequest{Queries: make([]baselineQuery, 0, len(corpus.Queries))}
	for i, gt := range corpus.Queries {
		req.Queries = append(req.Queries, baselineQuery{ID: i, Query: gt.Query})
	}
	body, err := json.Marshal(req)
	if err != nil {
		return BaselineArmReport{Configured: true, Error: err.Error()},
			fmt.Errorf("external baseline: failed to encode request: %w", err)
	}

	client := &http.Client{Timeout: baselineTimeout}
	resp, err := client.Post(url, "application/json", bytes.NewReader(body))
	if err != nil {
		return BaselineArmReport{Configured: true, Error: err.Error()},
			fmt.Errorf("external baseline: cannot reach %s (set %s to a reachable endpoint or unset it to skip): %w",
				url, BaselineURLEnv, err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return BaselineArmReport{Configured: true, Error: resp.Status},
			fmt.Errorf("external baseline: %s returned %s (set %s to a reachable endpoint or unset it to skip)",
				url, resp.Status, BaselineURLEnv)
	}

	var parsed baselineResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return BaselineArmReport{Configured: true, Error: err.Error()},
			fmt.Errorf("external baseline: malformed response from %s: %w", url, err)
	}

	byQuery := make(map[int][]string, len(parsed.Results))
	for _, r := range parsed.Results {
		byQuery[r.QueryID] = r.IDs
	}

	report := BaselineArmReport{Configured: true}
	var recall5Sum float64
	for i, gt := range corpus.Queries {
		if len(gt.RelevantIDs) == 0 {
			continue
		}
		relevant := make(map[string]bool)
		for _, id := range gt.RelevantIDs {
			relevant[id] = true
		}
		r5 := RecallAtK(byQuery[i], relevant, 5)
		recall5Sum += r5
		report.PerQuery = append(report.PerQuery, r5)
	}
	n := len(report.PerQuery)
	if n == 0 {
		n = 1
	}
	report.RecallAt5 = recall5Sum / float64(n)
	report.QueryCount = n
	return report, nil
}
