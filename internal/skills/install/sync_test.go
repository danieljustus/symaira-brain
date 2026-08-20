package install

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestCollectConflicts(t *testing.T) {
	statuses := []InstallStatus{
		{Target: render.TargetOpenCode, Name: "ok-skill", Status: StatusInSync},
		{Target: render.TargetOpenCode, Name: "conflict-a", Status: StatusConflict},
		{Target: render.TargetOpenCode, Name: "conflict-b", Status: StatusConflict},
		{Target: render.TargetOpenCode, Name: "stale-skill", Status: StatusStale},
		{Target: render.TargetOpenCode, Name: "harness-edit", Status: StatusHarnessChanged},
	}
	got := CollectConflicts(statuses)
	if len(got) != 2 {
		t.Fatalf("expected 2 conflicts, got %d", len(got))
	}
	names := map[string]bool{}
	for _, s := range got {
		names[s.Name] = true
	}
	if !names["conflict-a"] || !names["conflict-b"] {
		t.Fatalf("expected conflict-a and conflict-b, got %v", names)
	}
}

func TestSyncSkipsHarnessChanged(t *testing.T) {
	stales := []InstallStatus{
		{Target: render.TargetOpenCode, Name: "edited", Status: StatusHarnessChanged, Mode: ModeCopy},
	}
	results := Sync(stales, SyncOptions{ConflictPolicy: ConflictAbort, BaseDir: t.TempDir()})
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Action != "skipped" {
		t.Fatalf("expected skipped, got %s", results[0].Action)
	}
	if results[0].Error != "harness changed; use symskills pull" {
		t.Fatalf("unexpected error: %s", results[0].Error)
	}
}

func TestSyncPreferSourceReinstallsConflicts(t *testing.T) {
	stales := []InstallStatus{
		{Target: render.TargetOpenCode, Name: "conflict", Status: StatusConflict, Mode: ModeCopy, Error: "conflict in: SKILL.md"},
	}
	results := Sync(stales, SyncOptions{
		ConflictPolicy: ConflictPreferSource,
		LibraryDir:     "/nonexistent",
		BaseDir:        t.TempDir(),
	})
	// With a nonexistent library dir, LoadBundle fails → action should be "failed".
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Action != "failed" {
		t.Fatalf("expected failed (bundle load error), got %s", results[0].Action)
	}
}

func TestSyncDryRunPlans(t *testing.T) {
	stales := []InstallStatus{
		{Target: render.TargetOpenCode, Name: "stale", Status: StatusStale, Mode: ModeCopy},
	}
	results := Sync(stales, SyncOptions{
		ConflictPolicy: ConflictAbort,
		DryRun:         true,
		BaseDir:        t.TempDir(),
	})
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Action != "planned" {
		t.Fatalf("expected planned (dry run), got %s", results[0].Action)
	}
}

func TestSyncInSyncSkippedSilently(t *testing.T) {
	stales := []InstallStatus{
		{Target: render.TargetOpenCode, Name: "good", Status: StatusInSync},
	}
	results := Sync(stales, SyncOptions{BaseDir: t.TempDir()})
	if len(results) != 0 {
		t.Fatalf("expected 0 results for in-sync, got %d", len(results))
	}
}
