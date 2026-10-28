//go:build unix

package usage

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestCodexHistoryGlobalBounds(t *testing.T) {
	for _, mode := range []string{"combined-events", "subtree-bytes"} {
		t.Run(mode, func(t *testing.T) {
			dir := t.TempDir()
			for index, sub := range []string{"sessions", "archived_sessions"} {
				path := filepath.Join(dir, sub)
				if err := os.Mkdir(path, 0700); err != nil {
					t.Fatal(err)
				}
				if mode == "combined-events" {
					var data strings.Builder
					for i := 1; i <= 6000; i++ {
						fmt.Fprintf(&data, `{"type":"event_msg","timestamp":"2026-10-20T10:00:00Z","payload":{"type":"token_count","model":"gpt-5.4","info":{"last_token_usage":{"input_tokens":%d,"output_tokens":1}}}}`+"\n", i+6000*index)
					}
					if err := os.WriteFile(filepath.Join(path, fmt.Sprintf("%d.jsonl", index)), []byte(data.String()), 0600); err != nil {
						t.Fatal(err)
					}
				} else if index == 0 {
					for i := 0; i < 9; i++ {
						f, err := os.Create(filepath.Join(path, fmt.Sprintf("%d.jsonl", i)))
						if err != nil {
							t.Fatal(err)
						}
						if err := f.Truncate(4 << 20); err != nil {
							t.Fatal(err)
						}
						if err := f.Close(); err != nil {
							t.Fatal(err)
						}
					}
				}
			}
			h := ScanCodexHistory(context.Background(), dir, time.Now())
			if h.Status != "unavailable" || len(h.Daily) != 0 || len(h.Periods) != 0 {
				t.Fatalf("%+v", h)
			}
		})
	}
}
