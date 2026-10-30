//go:build unix

package usage

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestCodexHistoryConsentAndQuotaIndependence(t *testing.T) {
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "sessions"), 0700); err != nil {
		t.Fatal(err)
	}
	line := `{"type":"event_msg","timestamp":"` + time.Now().Add(-time.Second).UTC().Format(time.RFC3339Nano) + `","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":1,"output_tokens":0}}}}` + "\n"
	path := filepath.Join(root, "sessions", "session.jsonl")
	if err := os.WriteFile(path, []byte(line), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", t.TempDir())
	t.Setenv("CODEX_HOME", root)
	t.Setenv("ANTHROPIC_ADMIN_KEY", "")
	t.Setenv("CODEX_ACCESS_TOKEN", "")
	for _, mode := range []string{"enabled", "disabled", "nonexact", "unselected", "default", "quota-error"} {
		t.Run(mode, func(t *testing.T) {
			t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY", "1")
			t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY_ONLY", "1")
			if mode == "disabled" {
				t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY", "0")
			}
			if mode == "nonexact" {
				t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY", "true")
			}
			calls := 0
			client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
				calls++
				return &http.Response{StatusCode: 401, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(`{}`))}, nil
			})}
			keychain := func() (string, *time.Time) {
				if mode == "enabled" {
					t.Fatal("history-only keychain I/O")
				}
				return "", nil
			}
			ids := []string{"codex"}
			if mode == "unselected" {
				ids = []string{"grok"}
			}
			if mode == "default" {
				ids = nil
			}
			if mode == "quota-error" {
				t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY_ONLY", "0")
				t.Setenv("CODEX_ACCESS_TOKEN", "synthetic")
			}
			var providers []Provider
			if mode == "default" {
				providers = []Provider{NewCodexProvider(client)}
			} else {
				providers = providersForIDs(client, ids, keychain, nil)
			}
			// Construction only: the history source can appear after provider construction.
			if err := os.Rename(path, path+".hidden"); err != nil {
				t.Fatal(err)
			}
			if err := os.Rename(path+".hidden", path); err != nil {
				t.Fatal(err)
			}
			report := BuildReport(context.Background(), providers)
			p := report.Providers[0]
			want := mode == "enabled" || mode == "quota-error"
			if (p.History != nil) != want {
				t.Fatalf("%s: %+v", mode, p)
			}
			if want && (p.History.Status != "available") {
				t.Fatalf("%+v", p.History)
			}
			if mode == "quota-error" {
				if calls != 1 || p.Error == "" || p.Snapshot != nil {
					t.Fatalf("quota independence: %+v calls %d", p, calls)
				}
			} else if calls != 0 {
				t.Fatal("unexpected transport", calls)
			}
			encoded, err := json.Marshal(report)
			if err != nil {
				t.Fatal(err)
			}
			if strings.Contains(string(encoded), "synthetic-private-sentinel") || strings.Contains(string(encoded), root) {
				t.Fatal("private source leaked")
			}
			after, err := os.ReadFile(path)
			if err != nil || string(after) != line {
				t.Fatal("source mutated", err)
			}
		})
	}
}
