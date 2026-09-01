package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// newSkillsSandbox points every skills path at a temporary home and seeds a
// one-skill library, so the commands never read the developer's real library.
func newSkillsSandbox(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	t.Setenv("XDG_DATA_HOME", filepath.Join(home, ".local", "share"))
	t.Setenv("XDG_CACHE_HOME", filepath.Join(home, ".cache"))

	library := filepath.Join(home, ".local", "share", "symskills", "library", "demo-skill")
	if err := os.MkdirAll(library, 0o755); err != nil {
		t.Fatalf("create library: %v", err)
	}
	skillMD := "---\nname: demo-skill\ndescription: A demo skill for the CLI tests.\ncategory: testing\n---\n\nBody.\n"
	if err := os.WriteFile(filepath.Join(library, "SKILL.md"), []byte(skillMD), 0o644); err != nil {
		t.Fatalf("write SKILL.md: %v", err)
	}
	return home
}

func TestSkillsList_ReportsTheLibrary(t *testing.T) {
	newSkillsSandbox(t)

	t.Run("table", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		if code := run([]string{"skills", "list"}, &stdout, &stderr); code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		text := stdout.String()
		if !strings.Contains(text, "NAME\tCATEGORY\tINSTALLS\tDESCRIPTION") || !strings.Contains(text, "demo-skill") {
			t.Fatalf("unexpected table output: %q", text)
		}
	})

	t.Run("json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		if code := run([]string{"skills", "list", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		var report skillLibraryReport
		if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
			t.Fatalf("decode list JSON: %v (%q)", err, stdout.String())
		}
		if len(report.Skills) != 1 || report.Skills[0].Name != "demo-skill" {
			t.Fatalf("list JSON = %+v", report.Skills)
		}
		// The GUI keys installs off `path`; an empty one would break its
		// detail panel even though the row renders.
		if report.Skills[0].Path == "" {
			t.Error("list JSON carries no path for the skill")
		}
		if report.CategoryCounts["testing"] != 1 {
			t.Errorf("category_counts = %v, want testing:1", report.CategoryCounts)
		}
		// Always an array, never null — the GUI decodes it as a list.
		if report.Skills[0].Installs == nil {
			t.Error("installs = null, want an empty array")
		}
	})
}

func TestSkillsStatusAndSync_ReportEmptySandboxWithoutWriting(t *testing.T) {
	newSkillsSandbox(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "status", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("status exit = %v, stderr = %q", code, stderr.String())
	}
	var status skillStatusReport
	if err := json.Unmarshal(stdout.Bytes(), &status); err != nil {
		t.Fatalf("decode status JSON: %v (%q)", err, stdout.String())
	}
	if status.Installs == nil {
		t.Error("installs = null, want an empty array")
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "sync", "--dry-run", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("sync exit = %v, stderr = %q", code, stderr.String())
	}
	var sync skillSyncReport
	if err := json.Unmarshal(stdout.Bytes(), &sync); err != nil {
		t.Fatalf("decode sync JSON: %v (%q)", err, stdout.String())
	}
	if !sync.DryRun {
		t.Error("sync JSON dry_run = false, want true")
	}
	if sync.Results == nil {
		t.Error("results = null, want an empty array")
	}
}

func TestSkillsTargets_ListsEveryRegisteredHarness(t *testing.T) {
	newSkillsSandbox(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "targets", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var report skillTargetsReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode targets JSON: %v (%q)", err, stdout.String())
	}
	if len(report.Targets) == 0 {
		t.Fatal("targets JSON is empty; the registry should always report harnesses")
	}
	for _, target := range report.Targets {
		if target.Target == "" || target.EffectiveSkillRoot == "" {
			t.Fatalf("incomplete target row: %+v", target)
		}
	}
}

func TestSkillsLog_ReadsTheOperationLogNewestFirst(t *testing.T) {
	home := newSkillsSandbox(t)

	logPath := filepath.Join(home, ".local", "share", "symskills", "events.jsonl")
	logger := events.New(logPath, "test")
	logger.Record(events.Event{TS: "2026-08-01T10:00:00Z", Event: "install", Skill: "demo-skill", Target: "claude", Outcome: "ok", Actor: "cli"})
	logger.Record(events.Event{TS: "2026-08-02T10:00:00Z", Event: "render", Skill: "demo-skill", Target: "claude", Outcome: "ok", Actor: "cli"})

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "log", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var records []events.Event
	if err := json.Unmarshal(stdout.Bytes(), &records); err != nil {
		t.Fatalf("decode log JSON: %v (%q)", err, stdout.String())
	}
	if len(records) != 2 {
		t.Fatalf("log JSON = %+v, want 2 records", records)
	}
	if records[0].Event != "render" {
		t.Errorf("first record = %q, want the newest (render)", records[0].Event)
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "log", "--limit", "1", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	records = nil
	if err := json.Unmarshal(stdout.Bytes(), &records); err != nil {
		t.Fatalf("decode bounded log JSON: %v", err)
	}
	if len(records) != 1 {
		t.Fatalf("bounded log JSON = %+v, want 1 record", records)
	}
}

func TestSkillsDoctor_ReportsSandboxPaths(t *testing.T) {
	home := newSkillsSandbox(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "doctor", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var report skillsDoctorReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode doctor JSON: %v (%q)", err, stdout.String())
	}
	if report.Config == nil || !strings.HasPrefix(report.Config.LibraryDir, home) {
		t.Fatalf("library_dir = %+v, want a path under the sandbox home %q", report.Config, home)
	}
	if len(report.Targets) == 0 {
		t.Error("doctor JSON reports no targets")
	}
}

func TestSkillsRejectsUnknownSubcommandTargetAndScope(t *testing.T) {
	newSkillsSandbox(t)

	for name, args := range map[string][]string{
		"subcommand": {"skills", "nonsense"},
		"target":     {"skills", "status", "--target", "nonsense"},
		"scope":      {"skills", "status", "--scope", "nonsense"},
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

func TestSkillsHelpAdvertisesEverySubcommand(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "--help"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	for _, subcommand := range []string{"list", "status", "targets", "log", "sync", "doctor"} {
		if !strings.Contains(stdout.String(), "  "+subcommand) {
			t.Errorf("skills help does not advertise %q:\n%s", subcommand, stdout.String())
		}
	}
}

// The skills subcommands share one prelude (flag set, scope/target resolution,
// environment load, render). These tables exercise the paths the JSON-focused
// tests above leave out: the group usage, the flag guards, and every table
// renderer — including the empty-state wording a user meets on a fresh install.

func TestSkills_NoSubcommandPrintsUsage(t *testing.T) {
	newSkillsSandbox(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills"}, &stdout, &stderr); code != exitcodes.ExitNoInput {
		t.Fatalf("exit = %v, want %v", code, exitcodes.ExitNoInput)
	}
	// The usage goes to stderr here because no subcommand is a usage error,
	// unlike an explicit --help.
	for _, want := range []string{"symbrain skills", "list", "status", "targets", "log", "sync", "doctor"} {
		if !strings.Contains(stderr.String(), want) {
			t.Errorf("usage on stderr does not mention %q:\n%s", want, stderr.String())
		}
	}
	if stdout.Len() != 0 {
		t.Errorf("usage error wrote to stdout: %q", stdout.String())
	}
}

func TestSkills_RejectUnknownFlagPerSubcommand(t *testing.T) {
	newSkillsSandbox(t)

	for _, sub := range []string{"list", "status", "targets", "log", "sync", "doctor"} {
		t.Run(sub, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run([]string{"skills", sub, "--nonsense"}, &stdout, &stderr); code != exitcodes.ExitNoInput {
				t.Fatalf("exit = %v, want %v (stdout: %q)", code, exitcodes.ExitNoInput, stdout.String())
			}
			if stderr.Len() == 0 {
				t.Error("rejection produced no diagnostic on stderr")
			}
		})
	}
}

// TestSkills_TableOutputInEmptySandbox pins the human-readable rendering of
// every subcommand against a sandbox with a library but no installs. The empty
// states matter most: a bare header with no rows reads like a failure, so each
// command states its emptiness in prose instead.
func TestSkills_TableOutputInEmptySandbox(t *testing.T) {
	home := newSkillsSandbox(t)

	logger := events.New(filepath.Join(home, ".local", "share", "symskills", "events.jsonl"), "test")
	logger.Record(events.Event{TS: "2026-08-30T09:00:00Z", Event: "render", Skill: "demo-skill", Target: "claude", Outcome: "ok", Actor: "cli"})

	cases := map[string]struct {
		args     []string
		wantsAll []string
	}{
		"list": {
			args:     []string{"skills", "list"},
			wantsAll: []string{"NAME\tCATEGORY\tINSTALLS\tDESCRIPTION", "demo-skill", "testing"},
		},
		"status": {
			args:     []string{"skills", "status"},
			wantsAll: []string{"No installed skills found."},
		},
		"targets": {
			args:     []string{"skills", "targets"},
			wantsAll: []string{"TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT", "claude"},
		},
		"log": {
			args:     []string{"skills", "log"},
			wantsAll: []string{"WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME", "render", "demo-skill"},
		},
		"sync dry-run": {
			args:     []string{"skills", "sync", "--dry-run"},
			wantsAll: []string{"Every installed skill is in sync."},
		},
		"doctor": {
			args:     []string{"skills", "doctor"},
			wantsAll: []string{"library", "rendered", "cache", "log", "versioning"},
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			if code := run(tc.args, &stdout, &stderr); code != exitcodes.ExitOK {
				t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
			}
			for _, want := range tc.wantsAll {
				if !strings.Contains(stdout.String(), want) {
					t.Errorf("table output missing %q:\n%s", want, stdout.String())
				}
			}
			if stderr.Len() != 0 {
				t.Errorf("unexpected stderr: %q", stderr.String())
			}
		})
	}
}

// TestSkillsList_EmptyLibrary covers the branch where the library directory does
// not exist at all — the state right after install, before the first sync.
func TestSkillsList_EmptyLibrary(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	t.Setenv("XDG_DATA_HOME", filepath.Join(home, ".local", "share"))
	t.Setenv("XDG_CACHE_HOME", filepath.Join(home, ".cache"))

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "list"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "No skills in the library.") {
		t.Errorf("output = %q, want the empty-library message", stdout.String())
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "log"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("log exit = %v, stderr = %q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "No recorded skill operations.") {
		t.Errorf("log output = %q, want the empty-log message", stdout.String())
	}
}

// TestSkills_ScopeAndTargetAreHonoured checks that the two shared resolvers
// actually reach the work, not just that a bad value is rejected: a --target
// that parses but is ignored would silently scan every harness.
func TestSkills_ScopeAndTargetAreHonoured(t *testing.T) {
	newSkillsSandbox(t)

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "targets", "--scope", "project", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
	}
	var report skillTargetsReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode targets JSON: %v", err)
	}
	if len(report.Targets) == 0 {
		t.Fatal("project scope returned no targets")
	}

	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "status", "--target", "claude", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("scoped status exit = %v, stderr = %q", code, stderr.String())
	}
	var status skillStatusReport
	if err := json.Unmarshal(stdout.Bytes(), &status); err != nil {
		t.Fatalf("decode status JSON: %v", err)
	}
	for _, row := range status.Installs {
		if string(row.Target) != "claude" {
			t.Fatalf("--target claude returned a %s row: %+v", row.Target, row)
		}
	}
}

// TestSkills_UnmanagedInstallIsReportedAndNeverOverwritten covers the safety
// property that matters most in a harness skill root: a directory symbrain did
// not install is reported, counted, and left alone by sync. Without it a sync
// would silently overwrite a hand-written skill — and this is not theoretical,
// an unmanaged skill is exactly what makes a real `symbrain sync` stop.
func TestSkills_UnmanagedInstallIsReportedAndNeverOverwritten(t *testing.T) {
	home := newSkillsSandbox(t)

	foreign := filepath.Join(home, ".claude", "skills", "handwritten")
	if err := os.MkdirAll(foreign, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	body := "---\nname: handwritten\ndescription: Written by hand, not installed by symbrain.\n---\n\nMine.\n"
	marker := filepath.Join(foreign, "SKILL.md")
	if err := os.WriteFile(marker, []byte(body), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	if code := run([]string{"skills", "status", "--target", "claude", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("status exit = %v, stderr = %q", code, stderr.String())
	}
	var report skillStatusReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode status JSON: %v (%q)", err, stdout.String())
	}
	if report.Summary.Unmanaged != 1 {
		t.Fatalf("summary = %+v, want unmanaged=1", report.Summary)
	}
	var found bool
	for _, row := range report.Installs {
		if row.Name == "handwritten" {
			found = true
			if row.Status != install.StatusUnmanaged {
				t.Errorf("handwritten status = %q, want %q", row.Status, install.StatusUnmanaged)
			}
		}
	}
	if !found {
		t.Fatalf("status did not report the unmanaged skill: %+v", report.Installs)
	}

	// The table renderer must show it too — the JSON is for tooling, the table
	// is what a person reads before running a sync.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "status", "--target", "claude"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("status table exit = %v, stderr = %q", code, stderr.String())
	}
	for _, want := range []string{"TARGET\tSKILL\tSTATUS\tMODE\tPATH", "handwritten", string(install.StatusUnmanaged)} {
		if !strings.Contains(stdout.String(), want) {
			t.Errorf("status table missing %q:\n%s", want, stdout.String())
		}
	}

	// A real sync must not touch it.
	before, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "sync", "--target", "claude"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("sync exit = %v, stderr = %q", code, stderr.String())
	}
	after, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("ReadFile after sync: %v", err)
	}
	if string(before) != string(after) {
		t.Fatal("sync rewrote a skill it does not manage")
	}
}

// TestSkills_StaleInstallIsDetectedAndRepaired walks the whole reason the
// command exists: install a skill, let the library source drift away from it,
// and check that status names it stale and sync puts it back. The earlier tests
// only ever saw an empty install set, so the repair path — the one that writes
// into a harness directory — was never executed.
func TestSkills_StaleInstallIsDetectedAndRepaired(t *testing.T) {
	home := newSkillsSandbox(t)
	source := filepath.Join(home, ".local", "share", "symskills", "library", "demo-skill", "SKILL.md")

	// Install through the real sync pipeline rather than by hand, so the
	// markers and base snapshot are the ones production writes.
	var stdout, stderr bytes.Buffer
	if code := run([]string{"sync", "--project", t.TempDir(), "claude"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("initial sync exit = %v, stderr = %q", code, stderr.String())
	}

	assertStatus := func(want install.StatusKind) []install.InstallStatus {
		t.Helper()
		var out, errb bytes.Buffer
		if code := run([]string{"skills", "status", "--target", "claude", "--output", "json"}, &out, &errb); code != exitcodes.ExitOK {
			t.Fatalf("status exit = %v, stderr = %q", code, errb.String())
		}
		var report skillStatusReport
		if err := json.Unmarshal(out.Bytes(), &report); err != nil {
			t.Fatalf("decode status JSON: %v (%q)", err, out.String())
		}
		for _, row := range report.Installs {
			if row.Name == "demo-skill" {
				if row.Status != want {
					t.Fatalf("demo-skill status = %q, want %q", row.Status, want)
				}
				return report.Installs
			}
		}
		t.Fatalf("status did not report demo-skill: %+v", report.Installs)
		return nil
	}

	assertStatus(install.StatusInSync)

	// Change the library source so the installed copy falls behind.
	body, err := os.ReadFile(source)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if err := os.WriteFile(source, append(body, []byte("\nA second paragraph that only the library has.\n")...), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	assertStatus(install.StatusStale)

	// A dry run must report the plan and change nothing.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "sync", "--target", "claude", "--dry-run", "--output", "json"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("dry-run exit = %v, stderr = %q", code, stderr.String())
	}
	var planned skillSyncReport
	if err := json.Unmarshal(stdout.Bytes(), &planned); err != nil {
		t.Fatalf("decode sync JSON: %v (%q)", err, stdout.String())
	}
	if !planned.DryRun || len(planned.Results) == 0 || planned.Results[0].Action != "planned" {
		t.Fatalf("dry run = %+v, want a planned action", planned)
	}
	assertStatus(install.StatusStale)

	// The real sync repairs it, and says so in the table.
	stdout.Reset()
	stderr.Reset()
	if code := run([]string{"skills", "sync", "--target", "claude"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("sync exit = %v, stderr = %q", code, stderr.String())
	}
	for _, want := range []string{"TARGET\tSKILL\tACTION\tDETAIL", "demo-skill", "installed"} {
		if !strings.Contains(stdout.String(), want) {
			t.Errorf("sync table missing %q:\n%s", want, stdout.String())
		}
	}
	assertStatus(install.StatusInSync)
}
