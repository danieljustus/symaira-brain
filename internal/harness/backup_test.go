package harness

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestBackup_MissingFile_IsNoop(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "does-not-exist.json")

	backupPath, err := Backup(path)
	if err != nil {
		t.Fatalf("Backup(missing): %v", err)
	}
	if backupPath != "" {
		t.Errorf("backupPath = %q, want empty", backupPath)
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("ReadDir: %v", err)
	}
	if len(entries) != 0 {
		t.Errorf("dir has %d entries, want 0 (no backup should be created)", len(entries))
	}
}

func TestBackup_ExistingFile_CopiesContentAndPreservesOriginal(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	original := []byte(`{"hello":"world"}`)
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	backupPath, err := Backup(path)
	if err != nil {
		t.Fatalf("Backup: %v", err)
	}
	if backupPath == "" {
		t.Fatal("backupPath is empty, want a path")
	}
	if !strings.HasPrefix(backupPath, path+".bak.") {
		t.Errorf("backupPath = %q, want prefix %q", backupPath, path+".bak.")
	}

	backupContent, err := os.ReadFile(backupPath)
	if err != nil {
		t.Fatalf("ReadFile(backup): %v", err)
	}
	if string(backupContent) != string(original) {
		t.Errorf("backup content = %q, want %q", backupContent, original)
	}

	// The original file must be untouched by Backup itself.
	stillThere, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile(original): %v", err)
	}
	if string(stillThere) != string(original) {
		t.Errorf("original content changed: %q, want %q", stillThere, original)
	}
}

func TestBackup_PreservesFileMode(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	if err := os.WriteFile(path, []byte("{}"), 0o640); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	backupPath, err := Backup(path)
	if err != nil {
		t.Fatalf("Backup: %v", err)
	}

	info, err := os.Stat(backupPath)
	if err != nil {
		t.Fatalf("Stat(backup): %v", err)
	}
	if info.Mode().Perm() != 0o640 {
		t.Errorf("backup mode = %v, want %v", info.Mode().Perm(), os.FileMode(0o640))
	}
}

func TestBackup_RepeatedCallsSucceed(t *testing.T) {
	// Two backups issued within the same second share a timestamp. They must
	// use distinct collision suffixes so neither rollback point is overwritten.
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	if err := os.WriteFile(path, []byte("first"), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	first, err := Backup(path)
	if err != nil {
		t.Fatalf("Backup (first): %v", err)
	}
	if _, err := os.Stat(first); err != nil {
		t.Fatalf("first backup missing: %v", err)
	}

	if err := os.WriteFile(path, []byte("second"), 0o600); err != nil {
		t.Fatalf("WriteFile (second): %v", err)
	}
	second, err := Backup(path)
	if err != nil {
		t.Fatalf("Backup (second): %v", err)
	}
	if first == second {
		t.Fatalf("collision backups share path %q", first)
	}
	if got, err := os.ReadFile(first); err != nil || string(got) != "first" {
		t.Fatalf("first backup = %q, err=%v; want first", got, err)
	}
	if got, err := os.ReadFile(second); err != nil || string(got) != "second" {
		t.Fatalf("second backup = %q, err=%v; want second", got, err)
	}
}
