package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// newMemoryWriteTestDB returns the path of an empty memory database plus a
// seeded rule, so the write and inspection commands have a store of their own.
func newMemoryWriteTestDB(t *testing.T, rules ...*db.Rule) string {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/memory.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open memory database: %v", err)
	}
	for _, rule := range rules {
		if err := database.SaveRule(rule); err != nil {
			_ = database.Close()
			t.Fatalf("seed rule %s: %v", rule.ID, err)
		}
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close memory database: %v", err)
	}
	return cfg.Database.Path
}

func TestMemorySet_StoresThroughTheGovernedWritePath(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{
		"memory", "set", "prefers tabs over spaces",
		"--db", path, "--scope", "global", "--kind", "user", "--output", "json",
	}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}

	var result memorySetResult
	if err := json.Unmarshal(stdout.Bytes(), &result); err != nil {
		t.Fatalf("decode set JSON: %v (%q)", err, stdout.String())
	}
	if result.ID == "" {
		t.Fatalf("set JSON carries no id: %+v", result)
	}
	if result.Kind != "user" || result.Scope != "global" || result.Staged {
		t.Errorf("set JSON = %+v, want kind=user scope=global staged=false", result)
	}

	// The memory must be readable through the list command it shares a store
	// with — the write path and the read path agree on the database.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "list", "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("list exit = %v, stderr = %q", code, stderr.String())
	}
	var listed []*db.Memory
	if err := json.Unmarshal(stdout.Bytes(), &listed); err != nil {
		t.Fatalf("decode list JSON: %v (%q)", err, stdout.String())
	}
	if len(listed) != 1 || listed[0].ID != result.ID {
		t.Fatalf("list JSON = %+v, want the memory just written", listed)
	}
	if listed[0].Kind != "user" {
		t.Errorf("stored kind = %q, want user", listed[0].Kind)
	}
	if listed[0].CreatedBy != defaultMemoryAuthor {
		t.Errorf("stored author = %q, want %q", listed[0].CreatedBy, defaultMemoryAuthor)
	}
}

func TestMemorySet_StagedWriteIsHeldForReview(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{
		"memory", "set", "derived without confirmation",
		"--db", path, "--kind", "project", "--staged", "--output", "json",
	}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var result memorySetResult
	if err := json.Unmarshal(stdout.Bytes(), &result); err != nil {
		t.Fatalf("decode set JSON: %v (%q)", err, stdout.String())
	}
	if !result.Staged {
		t.Fatalf("set JSON = %+v, want staged=true", result)
	}

	cfg := config.Defaults()
	cfg.Database.Path = path
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("reopen memory database: %v", err)
	}
	defer func() { _ = database.Close() }()
	stored, err := database.GetMemory(result.ID)
	if err != nil || stored == nil {
		t.Fatalf("get memory %s: %v", result.ID, err)
	}
	if stored.ReviewStatus != db.ReviewStaged {
		t.Errorf("review status = %q, want %q", stored.ReviewStatus, db.ReviewStaged)
	}
}

func TestMemorySet_RejectsMissingAndInvalidKind(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	for name, args := range map[string][]string{
		"missing": {"memory", "set", "a fact", "--db", path},
		"invalid": {"memory", "set", "a fact", "--db", path, "--kind", "nonsense"},
		"noArgs":  {"memory", "set", "--db", path, "--kind", "user"},
	} {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(args, &stdout, &stderr); code != exitcodes.ExitNoInput {
				t.Fatalf("exit = %v, want %v (stdout %q)", code, exitcodes.ExitNoInput, stdout.String())
			}
			if stderr.Len() == 0 {
				t.Error("rejection produced no diagnostic on stderr")
			}
		})
	}
}

func TestMemoryDelete_RemovesTheMemory(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "set", "temporary", "--db", path, "--kind", "reference", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("set exit = %v, stderr = %q", code, stderr.String())
	}
	var written memorySetResult
	if err := json.Unmarshal(stdout.Bytes(), &written); err != nil {
		t.Fatalf("decode set JSON: %v", err)
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "delete", written.ID, "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("delete exit = %v, stderr = %q", code, stderr.String())
	}
	var deleted memoryDeleteResult
	if err := json.Unmarshal(stdout.Bytes(), &deleted); err != nil {
		t.Fatalf("decode delete JSON: %v (%q)", err, stdout.String())
	}
	if deleted.ID != written.ID || !deleted.Deleted {
		t.Fatalf("delete JSON = %+v, want the deleted id", deleted)
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "list", "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("list exit = %v, stderr = %q", code, stderr.String())
	}
	var listed []*db.Memory
	if err := json.Unmarshal(stdout.Bytes(), &listed); err != nil {
		t.Fatalf("decode list JSON: %v", err)
	}
	if len(listed) != 0 {
		t.Fatalf("list JSON = %+v, want an empty store after delete", listed)
	}
}

func TestMemoryRules_TableAndJSONWithRedaction(t *testing.T) {
	created := time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
	path := newMemoryWriteTestDB(t, &db.Rule{
		ID:        "rule-1",
		Scope:     "global",
		Content:   "always cc alice@example.com on releases",
		CreatedAt: created,
		UpdatedAt: created,
	})

	t.Run("table", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		if code := run([]string{"memory", "rules", "--db", path}, &stdout, &stderr); code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		text := stdout.String()
		if !strings.Contains(text, "ID\tSCOPE\tCREATED\tCONTENT") || !strings.Contains(text, "rule-1") {
			t.Fatalf("unexpected table output: %q", text)
		}
		if strings.Contains(text, "alice@example.com") {
			t.Fatalf("table output leaked unredacted rule content: %q", text)
		}
	})

	t.Run("json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		if code := run([]string{"memory", "rules", "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		var rules []*db.Rule
		if err := json.Unmarshal(stdout.Bytes(), &rules); err != nil {
			t.Fatalf("decode rules JSON: %v (%q)", err, stdout.String())
		}
		if len(rules) != 1 || rules[0].ID != "rule-1" {
			t.Fatalf("rules JSON = %+v", rules)
		}
		if strings.Contains(stdout.String(), "alice@example.com") {
			t.Fatalf("rules JSON leaked raw PII: %q", stdout.String())
		}
	})
}

func TestMemoryQueryLog_ReportsSummaryAndEntries(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	cfg := config.Defaults()
	cfg.Database.Path = path
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open memory database: %v", err)
	}
	if _, err := database.LogQuery("claude", "global", "sess-1", "memory_search", "tabs", "", 12); err != nil {
		_ = database.Close()
		t.Fatalf("log query: %v", err)
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close memory database: %v", err)
	}

	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "query-log", "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var summary db.QueryLogSummary
	if err := json.Unmarshal(stdout.Bytes(), &summary); err != nil {
		t.Fatalf("decode query-log JSON: %v (%q)", err, stdout.String())
	}
	if summary.TotalQueries != 1 {
		t.Errorf("total_queries = %d, want 1", summary.TotalQueries)
	}
	if summary.ToolBreakdown["memory_search"] != 1 {
		t.Errorf("tool_breakdown = %v, want memory_search:1", summary.ToolBreakdown)
	}
	if len(summary.RecentEntries) != 1 || summary.RecentEntries[0].Actor != "claude" {
		t.Fatalf("recent_entries = %+v", summary.RecentEntries)
	}
}

func TestMemoryHelpAdvertisesEverySubcommand(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "--help"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	for _, subcommand := range []string{"list", "search", "set", "delete", "rules", "query-log", "sync"} {
		if !strings.Contains(stdout.String(), "  "+subcommand) {
			t.Errorf("memory help does not advertise %q:\n%s", subcommand, stdout.String())
		}
	}
}
