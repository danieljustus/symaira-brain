//go:build unix

package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type historyCases struct {
	Now, Timezone string
	Cases         []struct {
		Name     string
		Files    map[string]string
		Status   string
		Excluded int
	}
}

func loadHistoryCases(t *testing.T) historyCases {
	t.Helper()
	data, err := os.ReadFile("testdata/grok-history-cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases historyCases
	if err = json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	return cases
}
func historyFixture(t *testing.T, files map[string]string) string {
	t.Helper()
	home := t.TempDir()
	for path, data := range files {
		path = filepath.Join(home, "sessions", path)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	return home
}
func TestGrokHistoryCases(t *testing.T) {
	cases := loadHistoryCases(t)
	at, err := time.Parse(time.RFC3339, cases.Now)
	if err != nil {
		t.Fatal(err)
	}
	at = at.In(time.FixedZone(cases.Timezone, 2*60*60))
	for _, c := range cases.Cases {
		t.Run(c.Name, func(t *testing.T) {
			h := ScanGrokHistory(context.Background(), historyFixture(t, c.Files), at)
			if h.Status != c.Status || h.ExcludedEvents != c.Excluded {
				t.Fatalf("status=%s excluded=%d", h.Status, h.ExcludedEvents)
			}
			if h.Attribution != "unattributed" || h.Scope != "local_machine" {
				t.Fatal(h)
			}
			encoded, err := json.Marshal(h)
			if err != nil {
				t.Fatal(err)
			}
			fmt.Printf("HISTORY_CASE %s %s\n", c.Name, encoded)
			if strings.Contains(string(encoded), "eventId") || strings.Contains(string(encoded), "inputTokens") {
				t.Fatal("raw event leaked")
			}
			if c.Name == "replays-subagents-forks-resume" {
				m := h.Periods[0].Models[0]
				if m.Events != 3 || *m.Tokens != "470" {
					t.Fatal(m)
				}
			}
			if c.Name == "true-zero" {
				m := h.Daily[0].Models[0]
				if m.Tokens == nil || *m.Tokens != "0" {
					t.Fatal(m)
				}
			}
			if c.Name == "missing-measurements" {
				m := h.Daily[0].Models[0]
				if m.Tokens != nil || m.MissingTokens != 1 {
					t.Fatal(m)
				}
			}
			if c.Name == "calendar-boundaries" {
				if len(h.Daily) != 5 || h.Daily[0].Key != "2026-09-29" || h.Periods[0].Models[0].Events != 5 {
					t.Fatal(h)
				}
			}
		})
	}
}
func TestHistoryLegacyContract(t *testing.T) {
	data := []byte(`{"schema_version":1,"providers":[{"id":"grok","display_name":"Grok","configured":false,"auth_status":{"status":"missing","detail":"none"}}]}`)
	var r Report
	if err := json.Unmarshal(data, &r); err != nil {
		t.Fatal(err)
	}
	out, err := json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(out), "history") {
		t.Fatal("absent history emitted")
	}
	if string(out) != string(data) {
		t.Fatalf("legacy encode changed: %s", out)
	}
	h := ScanGrokHistory(context.Background(), t.TempDir(), time.Now())
	r.Providers[0].History = h
	out, err = json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Report
	if err := json.Unmarshal(out, &decoded); err != nil || decoded.Providers[0].History.Status != "no_data" {
		t.Fatal(err)
	}
}
func TestGrokHistoryServiceIndependent(t *testing.T) {
	cases := loadHistoryCases(t)
	home := historyFixture(t, cases.Cases[0].Files)
	t.Setenv("GROK_HOME", home)
	t.Setenv("SYMBRAIN_USAGE_GROK_HISTORY", "1")
	p := &GrokProvider{status: AuthStatus{Status: "missing"}}
	r := BuildReport(context.Background(), []Provider{p})
	if r.Providers[0].History == nil || r.Providers[0].Configured {
		t.Fatal(r)
	}
	p.token = "synthetic"
	p.client = &httpClientHistory
	r = BuildReport(context.Background(), []Provider{p})
	if r.Providers[0].History == nil || r.Providers[0].Error == "" || r.Providers[0].Snapshot != nil {
		t.Fatal(r)
	}
	t.Setenv("SYMBRAIN_USAGE_GROK_HISTORY", "")
	r = BuildReport(context.Background(), []Provider{p})
	if r.Providers[0].History != nil {
		t.Fatal("disabled scan")
	}
	t.Setenv("SYMBRAIN_USAGE_GROK_HISTORY", "1")
	t.Setenv("GROK_HOME", "unsafe/../home")
	r = BuildReport(context.Background(), nil)
	if len(r.Providers) != 0 {
		t.Fatal(r)
	}
}
