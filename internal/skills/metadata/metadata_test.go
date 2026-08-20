package metadata

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func writeSkill(t *testing.T, root string) {
	t.Helper()
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "SKILL.md"), []byte("---\nname: alpha\ndescription: test\n---\n# Body\n"), 0o644); err != nil {
		t.Fatal(err)
	}
}

func writeEvents(t *testing.T, path string, evs ...events.Event) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	logger := events.New(path, "test")
	for _, ev := range evs {
		logger.Record(ev)
	}
}

func writeMarker(t *testing.T, dest, installed, renderedAt string) {
	t.Helper()
	if err := os.MkdirAll(dest, 0o755); err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(map[string]any{
		"schema_version": 1,
		"managed_by":     "symskills",
		"target":         "opencode",
		"name":           "alpha",
		"rendered_at":    renderedAt,
		"mode":           "symlink",
		"installed":      installed,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dest, markerFile), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestCollectFreshSkillDegrades(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)

	rec := Collect(root, "alpha", Options{})

	if rec.CreatedAt == "" {
		t.Error("expected created_at from the skill directory mtime")
	}
	if rec.ModifiedAt == "" {
		t.Error("expected modified_at from SKILL.md mtime")
	}
	if rec.LastRenderedAt != "" {
		t.Errorf("expected no last_rendered_at, got %q", rec.LastRenderedAt)
	}
	if len(rec.Installs) != 0 {
		t.Errorf("expected no installs, got %v", rec.Installs)
	}
	if rec.LastUsed != nil {
		t.Errorf("expected null last_used, got %v", *rec.LastUsed)
	}
	if rec.LastUsedSource != "" {
		t.Errorf("expected empty last_used_source, got %q", rec.LastUsedSource)
	}
}

func TestCollectInstallsAndRenderFromEvents(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	logPath := filepath.Join(t.TempDir(), "events.jsonl")

	renderTS := "2026-08-01T10:00:00Z"
	writeEvents(t, logPath,
		events.Event{Event: events.EventRender, Skill: "alpha", Target: "all", Outcome: events.OutcomeOK, TS: "2026-07-01T10:00:00Z"},
		events.Event{Event: events.EventRender, Skill: "alpha", Target: "opencode", Outcome: events.OutcomeOK, TS: renderTS},
		events.Event{Event: events.EventInstall, Skill: "alpha", Target: "opencode", Path: "/home/u/.config/opencode/skills/alpha", Outcome: events.OutcomeOK, TS: "2026-08-02T10:00:00Z"},
		events.Event{Event: events.EventProfileInstall, Skill: "alpha", Target: "claude", Path: "/home/u/.claude/skills/alpha", Outcome: events.OutcomeOK, TS: "2026-08-03T10:00:00Z"},
		events.Event{Event: events.EventInstall, Skill: "alpha", Target: "opencode", Outcome: events.OutcomeError, Error: "boom", TS: "2026-08-04T10:00:00Z"},
		events.Event{Event: events.EventInstall, Skill: "other", Target: "codex", Outcome: events.OutcomeOK, TS: "2026-08-05T10:00:00Z"},
	)

	rec := Collect(root, "alpha", Options{LogPath: logPath})

	if rec.LastRenderedAt != renderTS {
		t.Errorf("last_rendered_at = %q, want %q", rec.LastRenderedAt, renderTS)
	}
	if len(rec.Installs) != 2 {
		t.Fatalf("expected 2 installs, got %v", rec.Installs)
	}
	if rec.Installs[0].Target != "claude" || rec.Installs[0].Path != "/home/u/.claude/skills/alpha" || rec.Installs[0].InstalledAt != "2026-08-03T10:00:00Z" {
		t.Errorf("unexpected first install: %+v", rec.Installs[0])
	}
	if rec.Installs[1].Target != "opencode" || rec.Installs[1].Path != "/home/u/.config/opencode/skills/alpha" || rec.Installs[1].InstalledAt != "2026-08-02T10:00:00Z" {
		t.Errorf("unexpected second install: %+v", rec.Installs[1])
	}
	// Error outcomes are not installs; other skills are filtered out.
	if rec.LastUsed != nil {
		t.Errorf("expected null last_used, got %v", *rec.LastUsed)
	}
}

func TestCollectMarkerFallback(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	home := t.TempDir()

	rendered := filepath.Join(home, ".local", "share", "symskills", "rendered", "opencode", "alpha")
	if err := os.MkdirAll(rendered, 0o755); err != nil {
		t.Fatal(err)
	}
	renderTime := time.Date(2026, 6, 1, 12, 0, 0, 0, time.UTC)
	if err := os.Chtimes(rendered, renderTime, renderTime); err != nil {
		t.Fatal(err)
	}
	dest := filepath.Join(home, ".config", "opencode", "skills", "alpha")
	writeMarker(t, dest, "2026-06-02T12:00:00Z", rendered)

	rec := Collect(root, "alpha", Options{InstallOpt: install.Options{HomeDir: home, Scope: render.ScopeUser}})

	if len(rec.Installs) != 1 {
		t.Fatalf("expected 1 marker-derived install, got %v", rec.Installs)
	}
	if rec.Installs[0].Target != "opencode" || rec.Installs[0].Path != dest || rec.Installs[0].InstalledAt != "2026-06-02T12:00:00Z" {
		t.Errorf("unexpected install: %+v", rec.Installs[0])
	}
	if rec.LastRenderedAt != "2026-06-01T12:00:00Z" {
		t.Errorf("last_rendered_at = %q, want rendered-tree mtime", rec.LastRenderedAt)
	}
	if rec.LastUsed != nil {
		t.Errorf("expected null last_used, got %v", *rec.LastUsed)
	}
}

func TestCollectEventsPrecedeMarker(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	home := t.TempDir()
	logPath := filepath.Join(t.TempDir(), "events.jsonl")

	dest := filepath.Join(home, ".config", "opencode", "skills", "alpha")
	writeMarker(t, dest, "2020-01-01T00:00:00Z", "")
	writeEvents(t, logPath,
		events.Event{Event: events.EventInstall, Skill: "alpha", Target: "opencode", Path: dest, Outcome: events.OutcomeOK, TS: "2026-08-02T10:00:00Z"},
	)

	rec := Collect(root, "alpha", Options{
		LogPath:    logPath,
		InstallOpt: install.Options{HomeDir: home, Scope: render.ScopeUser},
	})

	if len(rec.Installs) != 1 {
		t.Fatalf("expected 1 install, got %v", rec.Installs)
	}
	if rec.Installs[0].InstalledAt != "2026-08-02T10:00:00Z" {
		t.Errorf("event-log install must win over the stale marker, got %+v", rec.Installs[0])
	}
}

func TestLastUsedFromAtime(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	home := t.TempDir()

	dest := filepath.Join(home, ".config", "opencode", "skills", "alpha")
	if err := os.MkdirAll(dest, 0o755); err != nil {
		t.Fatal(err)
	}
	skillMD := filepath.Join(dest, "SKILL.md")
	if err := os.WriteFile(skillMD, []byte("# alpha\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	writeMarker(t, dest, "2026-06-02T12:00:00Z", "")

	// A read after the install write: atime newer than mtime.
	written := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	accessed := written.Add(2 * time.Hour)
	if err := os.Chtimes(skillMD, accessed, written); err != nil {
		t.Fatal(err)
	}

	rec := Collect(root, "alpha", Options{InstallOpt: install.Options{HomeDir: home, Scope: render.ScopeUser}})

	if rec.LastUsed == nil {
		t.Fatal("expected last_used from a useful atime")
	}
	if rec.LastUsed.UTC().Format(time.RFC3339) != accessed.UTC().Format(time.RFC3339) {
		t.Errorf("last_used = %v, want %v", *rec.LastUsed, accessed)
	}
	if rec.LastUsedSource != "install_atime" {
		t.Errorf("last_used_source = %q, want install_atime", rec.LastUsedSource)
	}
}

func TestLastUsedNullWithoutUsefulAtime(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	home := t.TempDir()

	dest := filepath.Join(home, ".config", "opencode", "skills", "alpha")
	if err := os.MkdirAll(dest, 0o755); err != nil {
		t.Fatal(err)
	}
	skillMD := filepath.Join(dest, "SKILL.md")
	if err := os.WriteFile(skillMD, []byte("# alpha\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	writeMarker(t, dest, "2026-06-02T12:00:00Z", "")
	// atime equal to mtime (install write only) is not usage evidence.
	written := time.Date(2026, 6, 2, 12, 0, 0, 0, time.UTC)
	if err := os.Chtimes(skillMD, written, written); err != nil {
		t.Fatal(err)
	}

	rec := Collect(root, "alpha", Options{InstallOpt: install.Options{HomeDir: home, Scope: render.ScopeUser}})

	if rec.LastUsed != nil {
		t.Errorf("expected null last_used, got %v (source %q)", *rec.LastUsed, rec.LastUsedSource)
	}
}

func TestLastUsedFromRegisteredProbe(t *testing.T) {
	root := t.TempDir()
	writeSkill(t, root)
	home := t.TempDir()

	dest := filepath.Join(home, ".config", "opencode", "skills", "alpha")
	writeMarker(t, dest, "2026-06-02T12:00:00Z", "")

	probeTime := time.Date(2026, 8, 4, 9, 30, 0, 0, time.UTC)
	RegisterUsageProbe(UsageProbe{
		Name:   "harness_log",
		Lookup: func(installPath string) (time.Time, bool) { return probeTime, true },
	})
	defer resetUsageProbes()

	rec := Collect(root, "alpha", Options{InstallOpt: install.Options{HomeDir: home, Scope: render.ScopeUser}})

	if rec.LastUsed == nil {
		t.Fatal("expected last_used from the registered probe")
	}
	if rec.LastUsed.UTC().Format(time.RFC3339) != "2026-08-04T09:30:00Z" {
		t.Errorf("last_used = %v, want probe time", *rec.LastUsed)
	}
	if rec.LastUsedSource != "harness_log" {
		t.Errorf("last_used_source = %q, want harness_log", rec.LastUsedSource)
	}
}

func TestCollectIgnoresInstallScanWithoutHome(t *testing.T) {
	// A caller that knows no home (bare MCP Options{}) must not scan the
	// real user's install dirs, so Collect stays hermetic.
	root := t.TempDir()
	writeSkill(t, root)
	rec := Collect(root, "alpha", Options{})
	if len(rec.Installs) != 0 {
		t.Fatalf("expected no installs without a home, got %v", rec.Installs)
	}
}

func TestCollectMissingRoot(t *testing.T) {
	rec := Collect(filepath.Join(t.TempDir(), "nope"), "nope", Options{})
	if rec.CreatedAt != "" || rec.ModifiedAt != "" || rec.LastRenderedAt != "" {
		t.Errorf("expected full degradation for a missing root, got %+v", rec)
	}
	if rec.LastUsed != nil || len(rec.Installs) != 0 {
		t.Errorf("expected empty installs and null last_used, got %+v", rec)
	}
}
