//go:build unix

package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func TestClaudeHistoryCompleteJSONCorpus(t *testing.T) {
	data, err := os.ReadFile("testdata/claude-history-cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name     string          `json:"name"`
		Source   string          `json:"source"`
		Now      string          `json:"now"`
		Offset   int             `json:"offset_seconds"`
		Expected json.RawMessage `json:"expected"`
	}
	if err = json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			dir := t.TempDir()
			project := filepath.Join(dir, "projects", "p", "session", "subagents")
			if err := os.MkdirAll(project, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(project, "agent.jsonl")
			before := []byte(c.Source)
			if err := os.WriteFile(path, before, 0600); err != nil {
				t.Fatal(err)
			}
			now, err := time.Parse(time.RFC3339, c.Now)
			if err != nil {
				t.Fatal(err)
			}
			now = now.In(time.FixedZone("fixture", c.Offset))
			got := ScanClaudeHistory(context.Background(), dir, now)
			encoded, err := json.Marshal(got)
			if err != nil {
				t.Fatal(err)
			}
			var actual, want any
			if err = json.Unmarshal(encoded, &actual); err != nil {
				t.Fatal(err)
			}
			if err = json.Unmarshal(c.Expected, &want); err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(actual, want) {
				t.Fatalf("got %s\nwant %s", encoded, c.Expected)
			}
			after, err := os.ReadFile(path)
			if err != nil || !bytes.Equal(before, after) {
				t.Fatal("source mutation", err)
			}
			if bytes.Contains(encoded, []byte("synthetic-private-sentinel")) || bytes.Contains(encoded, []byte(dir)) {
				t.Fatal("private source leaked")
			}
		})
	}
}
