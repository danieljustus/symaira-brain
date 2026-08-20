package events

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestRecordAndReadRoundTrip(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l := New(path, "1.2.3")

	l.Record(Event{Event: EventImport, Skill: "alpha", Path: "/lib/alpha", Outcome: OutcomeOK, Actor: ActorCLI})
	l.Record(Event{Event: EventInstall, Skill: "alpha", Target: "opencode", Scope: "user", Mode: "symlink", Path: "/dest/alpha", Outcome: OutcomeOK, Actor: ActorMCP})
	// Error records keep the failure explainable.
	l.Record(Event{Event: EventUninstall, Skill: "beta", Outcome: OutcomeError, Error: "refusing to remove unmanaged skill", Actor: ActorCLI})

	records, err := l.Read(Filter{})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(records) != 3 {
		t.Fatalf("got %d records, want 3", len(records))
	}
	if records[0].Event != EventImport || records[0].Outcome != OutcomeOK || records[0].Actor != ActorCLI {
		t.Errorf("record 0 = %+v", records[0])
	}
	if records[1].Actor != ActorMCP {
		t.Errorf("record 1 actor = %q, want mcp", records[1].Actor)
	}
	if records[2].Outcome != OutcomeError || !strings.Contains(records[2].Error, "unmanaged") {
		t.Errorf("record 2 = %+v", records[2])
	}
	// Timestamp and tool version are stamped when absent.
	for i, r := range records {
		if _, err := time.Parse(time.RFC3339, r.TS); err != nil {
			t.Errorf("record %d ts %q is not RFC3339: %v", i, r.TS, err)
		}
		if !strings.HasSuffix(r.TS, "Z") {
			t.Errorf("record %d ts %q is not UTC", i, r.TS)
		}
		if r.ToolVersion != "1.2.3" {
			t.Errorf("record %d tool_version = %q, want 1.2.3", i, r.ToolVersion)
		}
	}
}

func TestJSONFieldNamesAreSnakeCase(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l := New(path, "")
	l.Record(Event{
		Event:        EventInstall,
		Skill:        "alpha",
		SkillVersion: "0.2.0",
		Target:       "opencode",
		Scope:        "user",
		Mode:         "symlink",
		Path:         "/x",
		SourceHash:   "abc",
		Outcome:      OutcomeOK,
		Error:        "boom",
		ToolVersion:  "9.9.9",
		Actor:        ActorClient,
	})
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read log: %v", err)
	}
	var keys map[string]any
	if err := json.Unmarshal(data, &keys); err != nil {
		t.Fatalf("unmarshal record %q: %v", data, err)
	}
	want := []string{"ts", "event", "skill", "skill_version", "target", "scope", "mode", "path", "source_hash", "outcome", "error", "tool_version", "actor"}
	if len(keys) != len(want) {
		t.Fatalf("record has %d keys %v, want %v", len(keys), keys, want)
	}
	for _, k := range want {
		if _, ok := keys[k]; !ok {
			t.Errorf("missing key %q in %v", k, keys)
		}
	}
}

func TestReadChronologicalAcrossRotation(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	// Padded paths make each record ~180 bytes, so with a 500-byte limit
	// the third record deterministically triggers exactly one rotation.
	l := NewWithLimit(path, "", 500)

	pad := strings.Repeat("x", 100)
	first := Event{Event: EventImport, Skill: "alpha", Path: pad, Outcome: OutcomeOK}
	second := Event{Event: EventInstall, Skill: "alpha", Target: "opencode", Path: pad, Outcome: OutcomeOK}
	third := Event{Event: EventUninstall, Skill: "alpha", Path: pad, Outcome: OutcomeOK}
	l.Record(first)
	l.Record(second)
	l.Record(third)

	records, err := l.Read(Filter{})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(records) != 3 {
		t.Fatalf("got %d records, want 3 (rotation must not lose records)", len(records))
	}
	if records[0].Event != first.Event || records[1].Event != second.Event || records[2].Event != third.Event {
		t.Fatalf("records out of order: %+v", records)
	}
	// At least one previous file must survive.
	if _, err := os.Stat(rotatedPath(path)); err != nil {
		t.Fatalf("rotated file %s missing after rotation: %v", rotatedPath(path), err)
	}
}

func TestRotationKeepsFilesBounded(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l := NewWithLimit(path, "", 400)

	for i := 0; i < 50; i++ {
		l.Record(Event{Event: EventImport, Skill: "alpha", Outcome: OutcomeOK})
	}
	records, err := l.Read(Filter{})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(records) == 0 {
		t.Fatal("rotation dropped every record")
	}
	// The two surviving generations are a contiguous suffix of the 50
	// writes in chronological order (no reordering, no interleaving).
	var lastTS time.Time
	for i, r := range records {
		ts, err := time.Parse(time.RFC3339Nano, r.TS)
		if err != nil {
			t.Fatalf("record %d ts %q: %v", i, r.TS, err)
		}
		if ts.Before(lastTS) {
			t.Fatalf("records out of chronological order at %d", i)
		}
		lastTS = ts
	}
	// Neither generation may grow far beyond the rotation limit.
	for _, p := range []string{path, rotatedPath(path)} {
		if st, err := os.Stat(p); err == nil && st.Size() > 400+512 {
			t.Fatalf("%s size %d exceeds rotation limit 400", p, st.Size())
		}
	}
}

func TestReadFilters(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l := New(path, "")
	base := time.Now().UTC().Truncate(time.Second)
	l.Record(Event{Event: EventImport, Skill: "alpha", Outcome: OutcomeOK, TS: base.Add(-2 * time.Hour).Format(time.RFC3339)})
	l.Record(Event{Event: EventInstall, Skill: "alpha", Target: "opencode", Outcome: OutcomeOK, TS: base.Add(-1 * time.Hour).Format(time.RFC3339)})
	l.Record(Event{Event: EventInstall, Skill: "beta", Target: "claude", Outcome: OutcomeOK, TS: base.Format(time.RFC3339)})

	bySkill, err := l.Read(Filter{Skill: "alpha"})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(bySkill) != 2 {
		t.Fatalf("skill filter: got %d records, want 2", len(bySkill))
	}
	for _, r := range bySkill {
		if r.Skill != "alpha" {
			t.Errorf("skill filter returned %+v", r)
		}
	}

	byTarget, err := l.Read(Filter{Target: "claude"})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(byTarget) != 1 || byTarget[0].Skill != "beta" {
		t.Fatalf("target filter: got %+v", byTarget)
	}

	bySince, err := l.Read(Filter{Since: base.Add(-90 * time.Minute)})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(bySince) != 2 || bySince[0].Event != EventInstall {
		t.Fatalf("since filter: got %+v", bySince)
	}

	combined, err := l.Read(Filter{Skill: "alpha", Target: "opencode"})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(combined) != 1 {
		t.Fatalf("combined filter: got %+v", combined)
	}
}

func TestRecordSwallowsUnwritableLog(t *testing.T) {
	// A directory at the log path makes the append open fail with EISDIR
	// even when running as root; Record must still not return an error and
	// a subsequent operation must be unaffected.
	base := t.TempDir()
	path := filepath.Join(base, "events.jsonl")
	if err := os.Mkdir(path, 0o755); err != nil {
		t.Fatal(err)
	}
	l := New(path, "")
	l.Record(Event{Event: EventInstall, Skill: "alpha", Outcome: OutcomeOK}) // must not panic or fail

	// A second logger pointed at a read-only directory degrades too.
	ro := filepath.Join(t.TempDir(), "ro")
	if err := os.MkdirAll(ro, 0o500); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(ro, 0o500); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(ro, 0o700)
	l2 := New(filepath.Join(ro, "events.jsonl"), "")
	l2.Record(Event{Event: EventImport, Skill: "alpha", Outcome: OutcomeOK})
}

func TestReadSkipsCorruptLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l := New(path, "")
	l.Record(Event{Event: EventImport, Skill: "alpha", Outcome: OutcomeOK})
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.WriteString("{torn json line\n"); err != nil {
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
	records, err := l.Read(Filter{})
	if err != nil {
		t.Fatalf("Read: %v", err)
	}
	if len(records) != 1 {
		t.Fatalf("got %d records, want 1 (corrupt line skipped)", len(records))
	}
}

func TestNilLoggerIsNoop(t *testing.T) {
	var l *Logger
	l.Record(Event{Event: EventImport, Outcome: OutcomeOK}) // must not panic
	records, err := l.Read(Filter{})
	if err != nil || records != nil {
		t.Fatalf("nil logger Read = %v, %v; want nil, nil", records, err)
	}
	if l.Path() != "" {
		t.Fatalf("nil logger Path = %q", l.Path())
	}
}

func TestDefaultPathUnderHome(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	want := filepath.Join(home, ".local", "share", "symskills", "events.jsonl")
	if got := DefaultPath(); got != want {
		t.Fatalf("DefaultPath() = %q, want %q", got, want)
	}
}
