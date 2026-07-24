package skills

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestSync_SymskillsAvailable(t *testing.T) {
	fakeScript := writeFakeSymskills(t, `{"results":[{"target":"claude","status":"ok","message":"2 skills rendered"}]}`)

	r := &Runner{
		LookPath: func(name string) (string, error) {
			if name == "symskills" {
				return fakeScript, nil
			}
			return "", fmt.Errorf("not found")
		},
		Run: func(name string, args ...string) ([]byte, error) {
			return runScript(fakeScript, args...)
		},
	}

	results, summary := r.Sync([]string{"claude", "codex"}, 5*time.Second)

	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	if results[0].Status != "ok" {
		t.Errorf("claude status = %q, want ok", results[0].Status)
	}
	if results[0].Message != "2 skills rendered" {
		t.Errorf("claude message = %q, want '2 skills rendered'", results[0].Message)
	}
	if results[1].Status != "ok" {
		t.Errorf("codex status = %q, want ok", results[1].Status)
	}
	if summary == "" {
		t.Error("summary should not be empty")
	}
}

func TestSync_SymskillsNotFound(t *testing.T) {
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "", fmt.Errorf("not found")
		},
	}

	results, summary := r.Sync([]string{"claude"}, 5*time.Second)

	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "skipped" {
		t.Errorf("status = %q, want skipped", results[0].Status)
	}
	if results[0].Message == "" {
		t.Error("expected a hint message about installing symskills")
	}
	if summary == "" {
		t.Error("summary should not be empty")
	}
}

func TestSync_UnsupportedHarness(t *testing.T) {
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "/usr/bin/symskills", nil
		},
		Run: func(name string, args ...string) ([]byte, error) {
			return []byte(`{"results":[]}`), nil
		},
	}

	results, summary := r.Sync([]string{"claude-desktop"}, 5*time.Second)

	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "skipped" {
		t.Errorf("status = %q, want skipped", results[0].Status)
	}
	if results[0].Message == "" {
		t.Error("expected an unsupported-harness message")
	}
	_ = summary
}

func TestSync_MixedResults(t *testing.T) {
	callCount := 0
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "/usr/bin/symskills", nil
		},
		Run: func(name string, args ...string) ([]byte, error) {
			callCount++
			if callCount == 1 {
				// First call (claude) succeeds.
				return []byte(`{"results":[{"target":"claude","status":"ok"}]}`), nil
			}
			// Second call (codex) fails.
			return nil, fmt.Errorf("symskills crashed")
		},
	}

	results, _ := r.Sync([]string{"claude", "codex"}, 5*time.Second)

	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	if results[0].Status != "ok" {
		t.Errorf("claude status = %q, want ok", results[0].Status)
	}
	if results[1].Status != "error" {
		t.Errorf("codex status = %q, want error", results[1].Status)
	}
}

func TestSync_EmptyTargets(t *testing.T) {
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "/usr/bin/symskills", nil
		},
		Run: func(name string, args ...string) ([]byte, error) {
			return []byte(`{"results":[]}`), nil
		},
	}

	results, summary := r.Sync(nil, 5*time.Second)

	if len(results) != 0 {
		t.Errorf("expected 0 results, got %d", len(results))
	}
	if summary != "" {
		t.Errorf("expected empty summary, got %q", summary)
	}
}

func TestAvailable_True(t *testing.T) {
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "/usr/bin/symskills", nil
		},
	}
	if !r.Available() {
		t.Error("Available() = false, want true")
	}
}

func TestAvailable_False(t *testing.T) {
	r := &Runner{
		LookPath: func(name string) (string, error) {
			return "", fmt.Errorf("not found")
		},
	}
	if r.Available() {
		t.Error("Available() = true, want false")
	}
}

// writeFakeSymskills creates a shell script that mimics symskills behavior.
func writeFakeSymskills(t *testing.T, output string) string {
	t.Helper()
	dir := t.TempDir()

	var script string
	if runtime.GOOS == "windows" {
		path := filepath.Join(dir, "symskills.bat")
		content := fmt.Sprintf("@echo %s\n", output)
		if err := os.WriteFile(path, []byte(content), 0o700); err != nil {
			t.Fatal(err)
		}
		script = path
	} else {
		path := filepath.Join(dir, "symskills")
		content := fmt.Sprintf("#!/bin/sh\necho '%s'\n", output)
		if err := os.WriteFile(path, []byte(content), 0o700); err != nil {
			t.Fatal(err)
		}
		script = path
	}
	return script
}

func runScript(script string, args ...string) ([]byte, error) {
	cmd := exec.Command(script, args...)
	return cmd.CombinedOutput()
}

func TestDefaultRunner(t *testing.T) {
	r := DefaultRunner()
	if r == nil {
		t.Fatal("expected non-nil Runner")
	}
	if r.LookPath == nil {
		t.Error("expected non-nil LookPath")
	}
	if r.Run == nil {
		t.Error("expected non-nil Run")
	}

	_, _ = r.LookPath("go")
	out, err := r.Run("go", "version")
	if err != nil || len(out) == 0 {
		t.Errorf("DefaultRunner.Run failed or empty output: err=%v, out=%s", err, string(out))
	}
}

