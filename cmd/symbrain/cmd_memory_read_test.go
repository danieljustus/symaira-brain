package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func newMemoryReadTestDB(t *testing.T, memories ...*db.Memory) string {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/memory.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open memory database: %v", err)
	}
	for _, m := range memories {
		if _, err := database.UpsertMemoryIfNewer(m); err != nil {
			_ = database.Close()
			t.Fatalf("seed memory %s: %v", m.ID, err)
		}
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close memory database: %v", err)
	}
	return cfg.Database.Path
}

func TestMemoryList_TableAndJSON(t *testing.T) {
	created := time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
	path := newMemoryReadTestDB(t,
		&db.Memory{ID: "global-1", Scope: "global", Content: "keep database contact alice@example.com", CreatedAt: created, UpdatedAt: created},
		&db.Memory{ID: "project-1", Scope: "project", Content: "other project fact", CreatedAt: created.Add(-time.Minute), UpdatedAt: created.Add(-time.Minute)},
	)

	t.Run("table", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"memory", "list", "--db", path, "--scope", "global", "--limit", "1"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		text := stdout.String()
		if !strings.Contains(text, "ID\tSCOPE\tCREATED\tCONTENT") || !strings.Contains(text, "global-1") {
			t.Fatalf("unexpected table output: %q", text)
		}
		if strings.Contains(text, "alice@example.com") {
			t.Fatalf("table output leaked unredacted content: %q", text)
		}
		if stderr.Len() != 0 {
			t.Errorf("unexpected stderr: %q", stderr.String())
		}
	})

	t.Run("json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"memory", "list", "--db", path, "--scope", "global", "--limit", "1", "--output", "json"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		var got []*db.Memory
		if err := json.Unmarshal(stdout.Bytes(), &got); err != nil {
			t.Fatalf("decode list JSON: %v (%q)", err, stdout.String())
		}
		if len(got) != 1 || got[0].ID != "global-1" {
			t.Fatalf("list JSON = %+v", got)
		}
		if got[0].Content != security.Redact("keep database contact alice@example.com") {
			t.Errorf("list JSON content = %q, want MCP redaction", got[0].Content)
		}
		if strings.Contains(stdout.String(), "alice@example.com") {
			t.Fatalf("list JSON leaked raw PII: %q", stdout.String())
		}
	})
}

func TestMemorySearch_TableAndJSON(t *testing.T) {
	const query = "database decision"
	created := time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	// Generate the fixture in the same embedding space the command will use.
	// This keeps the test deterministic whether Ollama is available or the
	// configured generator uses its local hash fallback.
	emb := extractor.NewEmbeddingsGenerator(memcfg).GenerateVector(query)
	path := newMemoryReadTestDB(t, &db.Memory{
		ID:              "decision-1",
		Scope:           "project",
		Content:         "database decision contact alice@example.com",
		Embedding:       emb.Vector,
		EmbeddingSource: emb.Source,
		CreatedAt:       created,
		UpdatedAt:       created,
	})

	t.Run("table", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"memory", "search", query, "--db", path, "--scope", "project", "--limit", "1"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		text := stdout.String()
		if !strings.Contains(text, "SCORE\tID\tSCOPE\tCREATED\tCONTENT") || !strings.Contains(text, "decision-1") {
			t.Fatalf("unexpected table output: %q", text)
		}
		if strings.Contains(text, "alice@example.com") {
			t.Fatalf("search table leaked raw PII: %q", text)
		}
	})

	t.Run("json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"memory", "search", query, "--db", path, "--scope", "project", "--limit", "1", "--json"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		var got []db.SearchResult
		if err := json.Unmarshal(stdout.Bytes(), &got); err != nil {
			t.Fatalf("decode search JSON: %v (%q)", err, stdout.String())
		}
		if len(got) != 1 || got[0].Memory == nil || got[0].Memory.ID != "decision-1" {
			t.Fatalf("search JSON = %+v", got)
		}
		if got[0].Memory.Content != security.Redact("database decision contact alice@example.com") {
			t.Errorf("search JSON content = %q, want MCP redaction", got[0].Memory.Content)
		}
		if strings.Contains(stdout.String(), "alice@example.com") {
			t.Fatalf("search JSON leaked raw PII: %q", stdout.String())
		}
	})
}

func TestMemoryReadHelpAndValidation(t *testing.T) {
	tests := []struct {
		name string
		args []string
		want string
	}{
		{"list help", []string{"memory", "list", "--help"}, "symbrain memory list"},
		{"search help", []string{"memory", "search", "--help"}, "symbrain memory search"},
		{"search missing query", []string{"memory", "search"}, "usage: symbrain memory search"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := run(tt.args, &stdout, &stderr)
			if strings.Contains(tt.name, "missing") {
				if code != exitcodes.ExitNoInput {
					t.Fatalf("exit = %v, want no input", code)
				}
				if !strings.Contains(stderr.String(), tt.want) {
					t.Fatalf("stderr %q missing %q", stderr.String(), tt.want)
				}
				return
			}
			if code != exitcodes.ExitOK || !strings.Contains(stdout.String(), tt.want) {
				t.Fatalf("exit = %v, stdout = %q, want %q", code, stdout.String(), tt.want)
			}
		})
	}
}
