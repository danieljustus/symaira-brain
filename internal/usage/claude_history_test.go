//go:build unix

package usage

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestClaudeHistoryCheckpointSelectedReport(t *testing.T) {
	dir := t.TempDir()
	projects := filepath.Join(dir, "projects")
	if err := os.Mkdir(projects, 0700); err != nil {
		t.Fatal(err)
	}
	now := time.Now().Add(-time.Second).UTC().Format(time.RFC3339Nano)
	line := `{"type":"assistant","timestamp":"` + now + `","requestId":"req-synthetic","message":{"id":"msg-synthetic","role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":40}}}` + "\n"
	if err := os.WriteFile(filepath.Join(projects, "session.jsonl"), []byte(line), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CLAUDE_CONFIG_DIR", dir)
	t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY", "1")
	t.Setenv("SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY", "1")
	providers, err := SelectedProviders(nil, "claude")
	if err != nil {
		t.Fatal(err)
	}
	report := BuildReport(context.Background(), providers)
	p := report.Providers[0]
	if p.Configured || p.History == nil || p.History.Status != "available" {
		t.Fatalf("unexpected report: %+v", p)
	}
	m := p.History.Daily[0].Models[0]
	if *m.Tokens != "190" {
		b, _ := json.Marshal(p.History)
		t.Fatal(string(b))
	}
}
