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

func TestClaudeHistoryConsentAndQuotaIndependence(t *testing.T) {
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "projects"), 0700); err != nil {
		t.Fatal(err)
	}
	line := `{"type":"assistant","timestamp":"` + time.Now().Add(-time.Second).UTC().Format(time.RFC3339Nano) + `","requestId":"r","message":{"id":"m","role":"assistant","model":"claude-opus-5-5","stop_reason":"end_turn","content":"synthetic-private-sentinel","usage":{"input_tokens":0,"output_tokens":0}}}` + "\n"
	path := filepath.Join(root, "projects", "session.jsonl")
	if err := os.WriteFile(path, []byte(line), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", t.TempDir())
	t.Setenv("CLAUDE_CONFIG_DIR", root)
	t.Setenv("ANTHROPIC_ADMIN_KEY", "")
	t.Setenv("ANTHROPIC_OAUTH_TOKEN", "")
	for _, mode := range []string{"enabled", "disabled", "nonexact", "unselected", "default", "quota-error"} {
		t.Run(mode, func(t *testing.T) {
			t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY", "1")
			t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY", "1")
			if mode == "disabled" {
				t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY", "0")
			}
			if mode == "nonexact" {
				t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY", "true")
			}
			calls := 0
			client := &http.Client{Transport: openRouterTestTransport(func(*http.Request) (*http.Response, error) {
				calls++
				return &http.Response{StatusCode: 401, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(`{}`))}, nil
			})}
			keychain := func() (string, *time.Time) {
				if mode == "enabled" {
					t.Fatal("history-only keychain I/O")
				}
				return "", nil
			}
			ids := []string{"claude"}
			if mode == "unselected" {
				ids = []string{"grok"}
			}
			if mode == "default" {
				ids = nil
			}
			if mode == "quota-error" {
				t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY", "0")
				t.Setenv("ANTHROPIC_OAUTH_TOKEN", "synthetic")
			}
			var providers []Provider
			if mode == "default" {
				providers = []Provider{newClaudeProvider(client, keychain)}
			} else {
				providers = providersForIDs(client, ids, keychain, nil)
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
