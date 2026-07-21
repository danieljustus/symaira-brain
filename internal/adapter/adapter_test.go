package adapter

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSync_Agents_FreshFile(t *testing.T) {
	dir := t.TempDir()
	content := "# Instructions\n\nGlobal rules here.\n"

	path, created, err := Sync(AgentsTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if !created {
		t.Fatal("Sync() should report created=true for a fresh file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "agents_fresh.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Agents_ExistingPreservesContent(t *testing.T) {
	dir := t.TempDir()

	// Pre-existing file with user content.
	existing := "# My Rules\n\nCustom stuff.\n"
	if err := os.WriteFile(filepath.Join(dir, "AGENTS.md"), []byte(existing), 0o600); err != nil {
		t.Fatal(err)
	}

	content := "# Instructions\n\nManaged rules.\n"
	path, created, err := Sync(AgentsTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if created {
		t.Fatal("Sync() should report created=false for an existing file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "agents_existing.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Claude_FreshFile(t *testing.T) {
	dir := t.TempDir()
	content := "# Instructions\n\nManaged content.\n"

	path, created, err := Sync(ClaudeTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if !created {
		t.Fatal("Sync() should report created=true for a fresh file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "claude_fresh.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Claude_ExistingPreservesContent(t *testing.T) {
	dir := t.TempDir()

	existing := "# My Custom Instructions\n\nUser content.\n"
	if err := os.WriteFile(filepath.Join(dir, "CLAUDE.md"), []byte(existing), 0o600); err != nil {
		t.Fatal(err)
	}

	content := "# Instructions\n\nManaged.\n"
	path, created, err := Sync(ClaudeTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if created {
		t.Fatal("Sync() should report created=false for an existing file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "claude_existing.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Cursor_FreshFile(t *testing.T) {
	dir := t.TempDir()
	content := "# Instructions\n\nRules.\n"

	path, created, err := Sync(CursorTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if !created {
		t.Fatal("Sync() should report created=true for a fresh file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "cursor_fresh.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Gemini_FreshFile(t *testing.T) {
	dir := t.TempDir()
	content := "# Instructions\n\nGemini rules.\n"

	path, created, err := Sync(GeminiTarget, content, dir)
	if err != nil {
		t.Fatalf("Sync() error: %v", err)
	}
	if !created {
		t.Fatal("Sync() should report created=true for a fresh file")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	golden := filepath.Join("testdata", "gemini_fresh.golden")
	assertGolden(t, golden, string(got))
}

func TestSync_Idempotent(t *testing.T) {
	dir := t.TempDir()
	content := "# Instructions\n\nIdempotent test.\n"

	// First sync.
	path1, _, err := Sync(AgentsTarget, content, dir)
	if err != nil {
		t.Fatal(err)
	}

	// Second sync — should produce identical bytes.
	path2, created, err := Sync(AgentsTarget, content, dir)
	if err != nil {
		t.Fatal(err)
	}
	if created {
		t.Fatal("second Sync() should report created=false")
	}
	if path1 != path2 {
		t.Fatalf("path changed: %q -> %q", path1, path2)
	}

	data1, _ := os.ReadFile(path1)
	data2, _ := os.ReadFile(path2)
	if string(data1) != string(data2) {
		t.Errorf("idempotency failed: first=%q second=%q", data1, data2)
	}
}

func assertGolden(t *testing.T, goldenPath, got string) {
	t.Helper()

	if _, err := os.Stat(goldenPath); os.IsNotExist(err) {
		if err := os.MkdirAll(filepath.Dir(goldenPath), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(goldenPath, []byte(got), 0o644); err != nil {
			t.Fatal(err)
		}
		t.Logf("golden file created: %s", goldenPath)
		return
	}

	want, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatal(err)
	}
	if got != string(want) {
		t.Errorf("golden mismatch\n--- got ---\n%s\n--- want (golden) ---\n%s", got, string(want))
	}
}
