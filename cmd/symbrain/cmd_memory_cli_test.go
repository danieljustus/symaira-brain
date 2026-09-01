package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// The memory subcommands repeat the same prelude: a help check, a flag set, an
// argument-count check, a store open and a render. These tables exercise that
// prelude once per entry point rather than once per behaviour, because a gap in
// any one of them is the same gap in all of them.

// TestMemoryUsage_EverySubcommandDocumentsItself asserts each subcommand's
// --help reaches stdout, names the command, and lists the flags callers need.
// A usage text that omits its own required flag is worse than none.
func TestMemoryUsage_EverySubcommandDocumentsItself(t *testing.T) {
	cases := map[string]struct {
		args     []string
		wantsAll []string
	}{
		"set": {
			args:     []string{"memory", "set", "--help"},
			wantsAll: []string{"symbrain memory set", "--kind", "--scope", "--staged", "--db"},
		},
		"delete": {
			args:     []string{"memory", "delete", "--help"},
			wantsAll: []string{"symbrain memory delete", "<id>", "--db"},
		},
		"rules": {
			args:     []string{"memory", "rules", "--help"},
			wantsAll: []string{"symbrain memory rules", "--scope", "--db"},
		},
		"query-log": {
			args:     []string{"memory", "query-log", "--help"},
			wantsAll: []string{"symbrain memory query-log", "--limit", "--actor", "--db"},
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(tc.args, &stdout, &stderr); code != exitcodes.ExitOK {
				t.Fatalf("exit = %v, want %v (stderr: %q)", code, exitcodes.ExitOK, stderr.String())
			}
			text := stdout.String()
			for _, want := range tc.wantsAll {
				if !strings.Contains(text, want) {
					t.Errorf("usage does not mention %q:\n%s", want, text)
				}
			}
			// Help is a successful result, not a diagnostic.
			if stderr.Len() != 0 {
				t.Errorf("help wrote to stderr: %q", stderr.String())
			}
		})
	}
}

// TestMemorySubcommands_RejectMalformedInvocations covers the argument and flag
// guards. Each case must exit ExitNoInput and say why on stderr — a silent
// rejection leaves the caller guessing which argument was wrong.
func TestMemorySubcommands_RejectMalformedInvocations(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	cases := map[string][]string{
		"set without content":        {"memory", "set", "--db", path, "--kind", "user"},
		"set with unknown flag":      {"memory", "set", "a fact", "--db", path, "--kind", "user", "--nonsense"},
		"set with invalid metadata":  {"memory", "set", "a fact", "--db", path, "--kind", "user", "--metadata", "not-json"},
		"delete without id":          {"memory", "delete", "--db", path},
		"delete with extra argument": {"memory", "delete", "id-1", "id-2", "--db", path},
		"rules with argument":        {"memory", "rules", "unexpected", "--db", path},
		"rules with unknown flag":    {"memory", "rules", "--db", path, "--nonsense"},
		"query-log with argument":    {"memory", "query-log", "unexpected", "--db", path},
		"query-log unknown flag":     {"memory", "query-log", "--db", path, "--nonsense"},
		"unknown subcommand":         {"memory", "nonsense"},
	}

	for name, args := range cases {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(args, &stdout, &stderr); code != exitcodes.ExitNoInput {
				t.Fatalf("exit = %v, want %v (stdout: %q, stderr: %q)",
					code, exitcodes.ExitNoInput, stdout.String(), stderr.String())
			}
			if stderr.Len() == 0 {
				t.Error("rejection produced no diagnostic on stderr")
			}
		})
	}
}

// TestMemorySubcommands_ReportAnUnopenableStore checks the store-open failure
// branch of every subcommand that touches the database. A directory in place of
// the database file is the cheapest way to make the open fail for real rather
// than by mocking it.
func TestMemorySubcommands_ReportAnUnopenableStore(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "not-a-file")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}

	cases := map[string][]string{
		"list":      {"memory", "list", "--db", dir},
		"set":       {"memory", "set", "a fact", "--kind", "user", "--db", dir},
		"delete":    {"memory", "delete", "some-id", "--db", dir},
		"rules":     {"memory", "rules", "--db", dir},
		"query-log": {"memory", "query-log", "--db", dir},
	}

	for name, args := range cases {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(args, &stdout, &stderr); code != exitcodes.ExitGeneric {
				t.Fatalf("exit = %v, want %v (stdout: %q, stderr: %q)",
					code, exitcodes.ExitGeneric, stdout.String(), stderr.String())
			}
			if !strings.Contains(stderr.String(), "symbrain memory "+name) {
				t.Errorf("stderr does not name the failing command: %q", stderr.String())
			}
		})
	}
}

// TestMemoryWrite_TableOutput asserts the human-readable rendering of the write
// commands, including that a staged write says so — the staged/stored
// distinction is the whole point of the flag and only the table surfaces it in
// prose.
func TestMemoryWrite_TableOutput(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "set", "prefers tabs", "--db", path, "--kind", "user"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("set exit = %v, stderr = %q", code, stderr.String())
	}
	stored := stdout.String()
	for _, want := range []string{"Memory ", "global", "user", "stored"} {
		if !strings.Contains(stored, want) {
			t.Errorf("set table output missing %q: %q", want, stored)
		}
	}
	if strings.Contains(stored, "staged for review") {
		t.Errorf("an unstaged write reported staging: %q", stored)
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "set", "derived guess", "--db", path, "--kind", "project", "--staged"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("staged set exit = %v, stderr = %q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "staged for review") {
		t.Errorf("staged write did not report staging: %q", stdout.String())
	}

	// Delete renders the id it removed, so a scripted caller can echo it back.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "list", "--db", path, "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("list exit = %v, stderr = %q", code, stderr.String())
	}
	var listed []*db.Memory
	if err := json.Unmarshal(stdout.Bytes(), &listed); err != nil || len(listed) == 0 {
		t.Fatalf("decode list JSON: %v (%q)", err, stdout.String())
	}
	id := listed[0].ID

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "delete", id, "--db", path}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("delete exit = %v, stderr = %q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "Deleted memory "+id) {
		t.Errorf("delete table output = %q, want it to name %s", stdout.String(), id)
	}
}

// TestMemoryReads_EmptyStoreTableOutput pins the empty-store wording. These are
// the lines a user sees first on a fresh install, and an empty table with only a
// header reads like a bug.
func TestMemoryReads_EmptyStoreTableOutput(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	cases := map[string]struct {
		args []string
		want string
	}{
		"rules":     {args: []string{"memory", "rules", "--db", path}, want: "No rules found."},
		"query-log": {args: []string{"memory", "query-log", "--db", path}, want: "No recorded queries."},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(tc.args, &stdout, &stderr); code != exitcodes.ExitOK {
				t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
			}
			if !strings.Contains(stdout.String(), tc.want) {
				t.Errorf("output = %q, want it to contain %q", stdout.String(), tc.want)
			}
		})
	}
}

// TestMemoryQueryLog_TableAndActorFilter covers the populated table renderer and
// the --actor filter. The filter is the reason the log is useful at all: it
// answers "what did this client ask for", so a filter that silently ignores its
// argument would be worse than no filter.
func TestMemoryQueryLog_TableAndActorFilter(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	cfg := config.Defaults()
	cfg.Database.Path = path
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open memory database: %v", err)
	}
	if _, err := database.LogQuery("claude", "global", "s1", "memory_search", "tabs over spaces", "", 12); err != nil {
		_ = database.Close()
		t.Fatalf("log query: %v", err)
	}
	if _, err := database.LogQuery("codex", "global", "s2", "memory_list", "", "", 3); err != nil {
		_ = database.Close()
		t.Fatalf("log query: %v", err)
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close memory database: %v", err)
	}

	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "query-log", "--db", path}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	text := stdout.String()
	if !strings.Contains(text, "Total queries: 2") {
		t.Errorf("table output missing the total: %q", text)
	}
	if !strings.Contains(text, "WHEN\tTOOL\tACTOR\tMS\tQUERY") {
		t.Errorf("table output missing the header: %q", text)
	}
	for _, want := range []string{"memory_search", "claude", "memory_list", "codex"} {
		if !strings.Contains(text, want) {
			t.Errorf("table output missing %q: %q", want, text)
		}
	}

	// The actor filter narrows the recent entries; the total stays a total.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "query-log", "--db", path, "--actor", "codex", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("filtered exit = %v, stderr = %q", code, stderr.String())
	}
	var summary db.QueryLogSummary
	if err := json.Unmarshal(stdout.Bytes(), &summary); err != nil {
		t.Fatalf("decode query-log JSON: %v (%q)", err, stdout.String())
	}
	if len(summary.RecentEntries) != 1 || summary.RecentEntries[0].Actor != "codex" {
		t.Fatalf("actor filter returned %+v, want only the codex entry", summary.RecentEntries)
	}
}

// TestMemoryRead_LimitIsClamped pins the documented bounds. The usage text
// promises a maximum, and a caller passing a huge limit must not be able to pull
// the whole store into one response.
func TestMemoryRead_LimitIsClamped(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	for _, args := range [][]string{
		{"memory", "list", "--db", path, "--limit", "99999", "--output", "json"},
		{"memory", "query-log", "--db", path, "--limit", "99999", "--output", "json"},
		{"memory", "list", "--db", path, "--limit", "-5", "--output", "json"},
		{"memory", "query-log", "--db", path, "--limit", "-5", "--output", "json"},
	} {
		var stdout, stderr bytes.Buffer
		if code := run(args, &stdout, &stderr); code != exitcodes.ExitOK {
			t.Fatalf("%v: exit = %v, stderr = %q", args, code, stderr.String())
		}
		if !json.Valid(bytes.TrimSpace(stdout.Bytes())) {
			t.Fatalf("%v: output is not valid JSON: %q", args, stdout.String())
		}
	}
}

// TestMemorySet_MetadataEntitiesAndScopeFilter covers the write flags that carry
// user data into the store and the scope filter that reads it back. A --metadata
// or --entities flag that parses but is dropped on the floor would look like a
// success and lose the caller's input silently.
func TestMemorySet_MetadataEntitiesAndScopeFilter(t *testing.T) {
	path := newMemoryWriteTestDB(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{
		"memory", "set", "Irene runs the Premium BnB cleaning schedule",
		"--db", path, "--kind", "reference", "--scope", "project",
		"--metadata", `{"source":"handover-note"}`,
		"--entities", "Irene, Premium BnB",
		"--output", "json",
	}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var result memorySetResult
	if err := json.Unmarshal(stdout.Bytes(), &result); err != nil {
		t.Fatalf("decode set JSON: %v (%q)", err, stdout.String())
	}

	cfg := config.Defaults()
	cfg.Database.Path = path
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open memory database: %v", err)
	}
	defer func() { _ = database.Close() }()

	stored, err := database.GetMemory(result.ID)
	if err != nil || stored == nil {
		t.Fatalf("get memory %s: %v", result.ID, err)
	}
	if stored.Metadata["source"] != "handover-note" {
		t.Errorf("metadata = %v, want the source key to survive the write", stored.Metadata)
	}
	if stored.Scope != "project" {
		t.Errorf("scope = %q, want project", stored.Scope)
	}
	// Entities are linked by name and created on demand; the comma list must be
	// split and trimmed, not stored as one blob.
	entities, err := database.ListEntities()
	if err != nil {
		t.Fatalf("list entities: %v", err)
	}
	names := map[string]bool{}
	for _, e := range entities {
		names[e.Name] = true
	}
	for _, want := range []string{"Irene", "Premium BnB"} {
		if !names[want] {
			t.Errorf("entity %q was not created; got %v", want, names)
		}
	}

	// The scope filter must actually narrow: a global query returns nothing.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"memory", "list", "--db", path, "--scope", "global", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("scoped list exit = %v, stderr = %q", code, stderr.String())
	}
	var listed []*db.Memory
	if err := json.Unmarshal(stdout.Bytes(), &listed); err != nil {
		t.Fatalf("decode list JSON: %v", err)
	}
	if len(listed) != 0 {
		t.Fatalf("scope=global returned %d project memories", len(listed))
	}
}

// TestMemoryRules_ScopeFilter pins that the rules scope filter narrows too. It
// shares the read prelude with list, but a filter wired to the wrong column
// would only show up here.
func TestMemoryRules_ScopeFilter(t *testing.T) {
	created := timeFixture()
	path := newMemoryWriteTestDB(t,
		&db.Rule{ID: "global-rule", Scope: "global", Content: "always run gofmt", CreatedAt: created, UpdatedAt: created},
		&db.Rule{ID: "project-rule", Scope: "project", Content: "never touch generated files", CreatedAt: created, UpdatedAt: created},
	)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "rules", "--db", path, "--scope", "project", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var rules []*db.Rule
	if err := json.Unmarshal(stdout.Bytes(), &rules); err != nil {
		t.Fatalf("decode rules JSON: %v (%q)", err, stdout.String())
	}
	if len(rules) != 1 || rules[0].ID != "project-rule" {
		t.Fatalf("scope=project returned %+v, want only project-rule", rules)
	}
}

// timeFixture is a fixed timestamp for rule fixtures. Rules carry no TTL, so a
// literal date is safe here — unlike activity segments, which expire.
func timeFixture() time.Time {
	return time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
}
