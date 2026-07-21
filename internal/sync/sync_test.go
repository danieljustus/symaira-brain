package sync

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/instructions"
)

func TestRun_GlobalAndProjectSources(t *testing.T) {
	home := t.TempDir()
	project := t.TempDir()

	// Set up global instructions in a temp config dir.
	// We can't easily override the xdg path, so we test via the
	// instructions.NewSource directly and pass content to Run via the
	// adapter targets.  Instead, test the orchestration end-to-end by
	// creating the expected directory structure.
	globalDir := filepath.Join(home, ".config", "symbrain")
	if err := os.MkdirAll(globalDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(globalDir, instructions.GlobalFileName), []byte("# Global Rules\n\nBe helpful.\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	// Create project-local instructions.
	projectInstrDir := filepath.Join(project, instructions.ProjectDirName)
	if err := os.MkdirAll(projectInstrDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(projectInstrDir, instructions.ProjectFileName), []byte("# Project Rules\n\nProject-specific.\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	// Override HOME so xdg resolves to our temp dir.
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", home)
	defer os.Setenv("HOME", origHome)

	var stderr bytes.Buffer
	statuses, _, err := Run(project, []string{"claude", "cursor"}, false, &stderr)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}

	if len(statuses) != 2 {
		t.Fatalf("expected 2 statuses, got %d", len(statuses))
	}

	for _, s := range statuses {
		if s.Status != "created" {
			t.Errorf("%s: status = %q, want created", s.Name, s.Status)
		}
	}

	// Verify CLAUDE.md was created.
	claudePath := filepath.Join(project, "CLAUDE.md")
	if _, err := os.Stat(claudePath); os.IsNotExist(err) {
		t.Error("CLAUDE.md was not created")
	}

	// Verify .cursor/rules/symbrain.mdc was created.
	cursorPath := filepath.Join(project, ".cursor", "rules", "symbrain.mdc")
	if _, err := os.Stat(cursorPath); os.IsNotExist(err) {
		t.Error(".cursor/rules/symbrain.mdc was not created")
	}

	// Second run should report all unchanged.
	statuses2, _, err := Run(project, []string{"claude", "cursor"}, false, &stderr)
	if err != nil {
		t.Fatalf("second Run() error: %v", err)
	}

	for _, s := range statuses2 {
		if s.Status != "unchanged" {
			t.Errorf("second run %s: status = %q, want unchanged", s.Name, s.Status)
		}
	}
}

func TestRun_DryRun(t *testing.T) {
	project := t.TempDir()

	var stderr bytes.Buffer
	statuses, _, err := Run(project, []string{"claude"}, true, &stderr)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}

	if len(statuses) != 1 {
		t.Fatalf("expected 1 status, got %d", len(statuses))
	}
	if statuses[0].Status != "dry-run" {
		t.Errorf("status = %q, want dry-run", statuses[0].Status)
	}

	// Verify no file was created.
	claudePath := filepath.Join(project, "CLAUDE.md")
	if _, err := os.Stat(claudePath); !os.IsNotExist(err) {
		t.Error("dry-run should not create files")
	}
}

func TestRun_UnsupportedHarness(t *testing.T) {
	project := t.TempDir()

	var stderr bytes.Buffer
	statuses, _, err := Run(project, []string{"claude-desktop"}, false, &stderr)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}

	if len(statuses) != 1 {
		t.Fatalf("expected 1 status, got %d", len(statuses))
	}
	if statuses[0].Status != "skipped" {
		t.Errorf("status = %q, want skipped", statuses[0].Status)
	}
}

func TestRun_AllHarnesses(t *testing.T) {
	project := t.TempDir()

	var stderr bytes.Buffer
	statuses, _, err := Run(project, nil, false, &stderr)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}

	// Should have statuses for all registered harnesses.
	if len(statuses) == 0 {
		t.Fatal("expected at least 1 status")
	}

	for _, s := range statuses {
		if s.Status == "" {
			t.Errorf("%s: empty status", s.Name)
		}
	}
}

func TestFormatSummary(t *testing.T) {
	statuses := []TargetStatus{
		{Name: "claude", Path: "/tmp/CLAUDE.md", Status: "created"},
		{Name: "cursor", Path: "/tmp/.cursor/rules/symbrain.mdc", Status: "updated"},
		{Name: "gemini", Path: "/tmp/GEMINI.md", Status: "unchanged"},
	}

	var buf bytes.Buffer
	FormatSummary(&buf, statuses, nil)

	out := buf.String()
	if !strings.Contains(out, "claude:") || !strings.Contains(out, "created") {
		t.Errorf("summary missing claude:created\n%s", out)
	}
	if !strings.Contains(out, "cursor:") || !strings.Contains(out, "updated") {
		t.Errorf("summary missing cursor:updated\n%s", out)
	}
	if !strings.Contains(out, "gemini:") || !strings.Contains(out, "unchanged") {
		t.Errorf("summary missing gemini:unchanged\n%s", out)
	}
}

func TestFormatSummaryJSON(t *testing.T) {
	statuses := []TargetStatus{
		{Name: "claude", Path: "/tmp/CLAUDE.md", Status: "created"},
	}

	var buf bytes.Buffer
	if err := FormatSummaryJSON(&buf, statuses, nil); err != nil {
		t.Fatalf("FormatSummaryJSON() error: %v", err)
	}

	out := buf.String()
	if !strings.Contains(out, `"name":"claude"`) {
		t.Errorf("JSON missing name:claude\n%s", out)
	}
	if !strings.Contains(out, `"status":"created"`) {
		t.Errorf("JSON missing status:created\n%s", out)
	}
}
