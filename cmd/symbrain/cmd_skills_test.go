package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
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
