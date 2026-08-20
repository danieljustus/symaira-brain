package mcptools

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// mcpResyncSetup builds an isolated skill + options for the resyncStaleMCP
// error-path tests. HOME is pointed at a temp dir: resyncStaleMCP does not
// propagate opts.HomeDir into install.Install, which resolves the skill root
// through os.UserHomeDir().
func mcpResyncSetup(t *testing.T, manifest string) (Options, install.InstallStatus) {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	lib := t.TempDir()
	bundleRoot := filepath.Join(lib, "resync-skill")
	if err := os.MkdirAll(bundleRoot, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(bundleRoot, "SKILL.md"), []byte("---\nname: resync-skill\ndescription: test\n---\n\nbody\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if manifest != "" {
		if err := os.WriteFile(filepath.Join(bundleRoot, "symskills.toml"), []byte(manifest), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	opts := Options{HomeDir: home, LibraryDir: lib, RenderDir: t.TempDir()}
	st := install.InstallStatus{Target: render.TargetOpenCode, Name: "resync-skill", Mode: "copy", Status: install.StatusStale}
	return opts, st
}

func mcpLogger(t *testing.T) *events.Logger {
	t.Helper()
	return events.New(filepath.Join(t.TempDir(), "events.jsonl"), "test")
}

func TestResyncStaleMCPBundleMissing(t *testing.T) {
	opts, _ := mcpResyncSetup(t, "")
	rows := resyncStaleMCP(opts, []install.InstallStatus{{Target: render.TargetOpenCode, Name: "missing", Mode: "copy", Status: install.StatusStale}}, mcpLogger(t))
	if len(rows) != 1 || rows[0]["action"] != "failed" || rows[0]["error"] == nil {
		t.Fatalf("expected failed row, got %+v", rows)
	}
}

func TestResyncStaleMCPNoRenderOutput(t *testing.T) {
	opts, st := mcpResyncSetup(t, `
[skill]
name = "resync-skill"

[targets.opencode]
enabled = false
`)
	logger := mcpLogger(t)
	rows := resyncStaleMCP(opts, []install.InstallStatus{st}, logger)
	if len(rows) != 1 || rows[0]["action"] != "failed" || !strings.Contains(rows[0]["error"].(string), "disabled") {
		t.Fatalf("expected disabled-target failure, got %+v", rows)
	}
	evs, err := logger.Read(events.Filter{})
	if err != nil {
		t.Fatal(err)
	}
	if len(evs) != 1 || evs[0].Outcome != events.OutcomeError {
		t.Fatalf("expected one error event, got %+v", evs)
	}
}

func TestResyncStaleMCPLockHeld(t *testing.T) {
	opts, st := mcpResyncSetup(t, "")
	lockPath := filepath.Join(opts.HomeDir, ".local", "share", "symskills", "pending", ".locks", "opencode", "resync-skill.lock")
	if err := os.MkdirAll(filepath.Dir(lockPath), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(lockPath, []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	rows := resyncStaleMCP(opts, []install.InstallStatus{st}, mcpLogger(t))
	if len(rows) != 1 || rows[0]["action"] != "skipped" || !strings.Contains(rows[0]["error"].(string), "pull lock held") {
		t.Fatalf("expected lock skip, got %+v", rows)
	}
}

func TestResyncStaleMCPInstallFailure(t *testing.T) {
	opts, st := mcpResyncSetup(t, "")
	// Sabotage the destination parent so Install's prepareDest fails.
	cfgPath := filepath.Join(opts.HomeDir, ".config", "opencode")
	if err := os.MkdirAll(filepath.Dir(cfgPath), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(cfgPath, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	logger := mcpLogger(t)
	rows := resyncStaleMCP(opts, []install.InstallStatus{st}, logger)
	if len(rows) != 1 || rows[0]["action"] != "failed" || rows[0]["error"] == nil {
		t.Fatalf("expected install failure row, got %+v", rows)
	}
	evs, err := logger.Read(events.Filter{})
	if err != nil {
		t.Fatal(err)
	}
	if len(evs) != 1 || evs[0].Outcome != events.OutcomeError {
		t.Fatalf("expected one error event, got %+v", evs)
	}
}

func TestResyncStaleMCPHappyPath(t *testing.T) {
	opts, st := mcpResyncSetup(t, "")
	rows := resyncStaleMCP(opts, []install.InstallStatus{st}, mcpLogger(t))
	if len(rows) != 1 || rows[0]["action"] == "failed" {
		t.Fatalf("expected success row, got %+v", rows)
	}
	if rows[0]["path"] == nil {
		t.Fatalf("expected installed path in row, got %+v", rows[0])
	}
}

func TestInstallProfileInvalidName(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home, LibraryDir: t.TempDir(), ProfilesDir: t.TempDir(), RenderDir: t.TempDir()}
	cfg := &config.Config{LibraryDir: opts.LibraryDir, ProfilesDir: opts.ProfilesDir, RenderDir: opts.RenderDir}
	logger := events.New(filepath.Join(home, "events.jsonl"), "test")
	_, err := installProfile(opts, cfg, render.TargetOpenCode, "a/b", install.Options{Scope: render.ScopeUser, Mode: install.ModeCopy}, logger)
	if err == nil {
		t.Fatal("expected invalid-profile-name error")
	}
	evs, rerr := logger.Read(events.Filter{})
	if rerr != nil {
		t.Fatal(rerr)
	}
	if len(evs) != 1 || evs[0].Event != events.EventProfileInstall || evs[0].Outcome != events.OutcomeError {
		t.Fatalf("expected one profile-install error event, got %+v", evs)
	}
}

func TestInstallProfileMissingLinkedSkill(t *testing.T) {
	home := t.TempDir()
	profilesDir := t.TempDir()
	// A profile that links a skill which does not exist in the library
	// produces validation issues, which installProfile reports without
	// failing.
	profile := `name = "broken"

[links.missing]
skill = "no-such-skill"
`
	if err := os.WriteFile(filepath.Join(profilesDir, "broken.toml"), []byte(profile), 0o644); err != nil {
		t.Fatal(err)
	}
	opts := Options{HomeDir: home, LibraryDir: t.TempDir(), ProfilesDir: profilesDir, RenderDir: t.TempDir()}
	cfg := &config.Config{LibraryDir: opts.LibraryDir, ProfilesDir: opts.ProfilesDir, RenderDir: opts.RenderDir}
	logger := events.New(filepath.Join(home, "events.jsonl"), "test")
	out, err := installProfile(opts, cfg, render.TargetOpenCode, "broken", install.Options{Scope: render.ScopeUser, Mode: install.ModeCopy}, logger)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if out == nil || !strings.Contains(out.(string), "issues") {
		t.Fatalf("expected issues payload, got %v", out)
	}
	evs, rerr := logger.Read(events.Filter{})
	if rerr != nil {
		t.Fatal(rerr)
	}
	if len(evs) != 1 || evs[0].Outcome != events.OutcomeError {
		t.Fatalf("expected one error event for issues, got %+v", evs)
	}
}

func TestInstallProfileDryRunBrokenLibrarySkill(t *testing.T) {
	home := t.TempDir()
	profilesDir := t.TempDir()
	lib := t.TempDir()
	// The profile resolves, but the linked library skill is a directory
	// without SKILL.md, so LoadBundle fails during the dry-run walk.
	brokenSkill := filepath.Join(lib, "broken-skill")
	if err := os.MkdirAll(brokenSkill, 0o755); err != nil {
		t.Fatal(err)
	}
	profile := `name = "dryrun-broken"

[links.missing]
skill = "broken-skill"
`
	if err := os.WriteFile(filepath.Join(profilesDir, "dryrun-broken.toml"), []byte(profile), 0o644); err != nil {
		t.Fatal(err)
	}
	opts := Options{HomeDir: home, LibraryDir: lib, ProfilesDir: profilesDir, RenderDir: t.TempDir()}
	_, err := installProfileDryRun(opts, render.TargetOpenCode, "dryrun-broken", install.Options{Scope: render.ScopeUser, Mode: install.ModeCopy})
	if err == nil {
		t.Fatal("expected load-bundle error for broken library skill")
	}
}
