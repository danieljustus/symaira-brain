package bench

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"
)

func setBaselineURL(t *testing.T, url string) {
	t.Helper()
	old, had := os.LookupEnv(BaselineURLEnv)
	if url == "" {
		_ = os.Unsetenv(BaselineURLEnv)
	} else {
		_ = os.Setenv(BaselineURLEnv, url)
	}
	t.Cleanup(func() {
		if had {
			_ = os.Setenv(BaselineURLEnv, old)
		} else {
			_ = os.Unsetenv(BaselineURLEnv)
		}
	})
}

func TestExternalBaseline_SkippedWhenUnconfigured(t *testing.T) {
	setBaselineURL(t, "")
	report, err := RunExternalBaseline(DefaultCorpus())
	if err != nil {
		t.Fatalf("unconfigured arm must not error: %v", err)
	}
	if !report.Skipped || report.Configured {
		t.Errorf("expected skipped/unconfigured report, got %+v", report)
	}
}

func TestExternalBaseline_ComputesRecall(t *testing.T) {
	corpus := DefaultCorpus()

	// Serve the corpus ground truth back as retrieved ids — a perfect
	// baseline scores Recall@5 = 1.0 on every answerable query.
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var req baselineRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			t.Errorf("bad request: %v", err)
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		resp := baselineResponse{}
		for _, q := range req.Queries {
			var ids []string
			if q.ID < len(corpus.Queries) {
				ids = corpus.Queries[q.ID].RelevantIDs
			}
			resp.Results = append(resp.Results, baselineResult{QueryID: q.ID, IDs: ids})
		}
		_ = json.NewEncoder(w).Encode(resp)
	}))
	defer server.Close()

	setBaselineURL(t, server.URL)
	report, err := RunExternalBaseline(corpus)
	if err != nil {
		t.Fatalf("baseline arm failed: %v", err)
	}
	if !report.Configured || report.Skipped {
		t.Fatalf("expected configured report, got %+v", report)
	}
	if report.RecallAt5 != 1.0 {
		t.Errorf("perfect baseline should score Recall@5 = 1.0, got %f", report.RecallAt5)
	}
	if report.QueryCount == 0 {
		t.Error("expected answerable queries to be counted")
	}
}

func TestExternalBaseline_FailsLoudlyWhenUnreachable(t *testing.T) {
	setBaselineURL(t, "http://127.0.0.1:1/unreachable") // port 1: nothing listens
	_, err := RunExternalBaseline(DefaultCorpus())
	if err == nil {
		t.Fatal("configured but unreachable endpoint must fail loudly")
	}
	if !strings.Contains(err.Error(), BaselineURLEnv) {
		t.Errorf("error must name the env var for an actionable fix: %v", err)
	}
}

func TestExternalBaseline_FailsLoudlyOnMalformedResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte("not json"))
	}))
	defer server.Close()

	setBaselineURL(t, server.URL)
	_, err := RunExternalBaseline(DefaultCorpus())
	if err == nil {
		t.Fatal("malformed response must fail loudly")
	}
}

func TestExternalBaseline_TimeoutBounded(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(2 * time.Second) // longer than the per-call budget in a fast test
	}))
	defer server.Close()

	setBaselineURL(t, server.URL)
	start := time.Now()
	_, err := RunExternalBaseline(DefaultCorpus())
	elapsed := time.Since(start)
	if err == nil {
		t.Fatal("hanging endpoint must error")
	}
	if elapsed > 5*time.Second {
		t.Errorf("baseline call not bounded: took %v", elapsed)
	}
}
