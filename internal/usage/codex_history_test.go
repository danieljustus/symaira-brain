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

func TestCodexHistoryCheckpointSelectedReport(t *testing.T) {
	dir := t.TempDir()
	sessions := filepath.Join(dir, "sessions")
	if err := os.Mkdir(sessions, 0700); err != nil {
		t.Fatal(err)
	}
	now := time.Now().Add(-time.Second).UTC().Format(time.RFC3339Nano)
	line := `{"type":"turn_context","payload":{"model":"gpt-5.4"}}` + "\n" + `{"type":"event_msg","timestamp":"` + now + `","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":20,"reasoning_output_tokens":10,"total_tokens":120}}}}` + "\n"
	if err := os.WriteFile(filepath.Join(sessions, "session.jsonl"), []byte(line), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", dir)
	t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY", "1")
	t.Setenv("SYMBRAIN_USAGE_CODEX_HISTORY_ONLY", "1")
	providers, err := SelectedProviders(nil, "codex")
	if err != nil {
		t.Fatal(err)
	}
	report := BuildReport(context.Background(), providers)
	p := report.Providers[0]
	if p.Configured || p.History == nil || p.History.Status != "available" {
		t.Fatalf("unexpected report: %+v", p)
	}
	m := p.History.Daily[0].Models[0]
	if *m.Tokens != "120" {
		b, _ := json.Marshal(p.History)
		t.Fatal(string(b))
	}
}
