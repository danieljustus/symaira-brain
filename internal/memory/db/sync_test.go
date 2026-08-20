package db

import (
	"os"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestGetSyncCursorZeroTime(t *testing.T) {
	tempDir := t.TempDir()
	oldHomeEnv := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHomeEnv)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// First read for a remote that has never been stored should return zero time.
	cursor, err := database.GetSyncCursor("https://example.com/repo")
	if err != nil {
		t.Fatalf("GetSyncCursor failed: %v", err)
	}
	if !cursor.IsZero() {
		t.Errorf("expected zero time for first read, got %v", cursor)
	}
}

func TestSyncCursorRoundTrip(t *testing.T) {
	tempDir := t.TempDir()
	oldHomeEnv := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHomeEnv)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	remote := "https://github.com/owner/project"
	want := time.Date(2026, 6, 1, 12, 30, 0, 0, time.UTC)

	if err := database.SetSyncCursor(remote, want); err != nil {
		t.Fatalf("SetSyncCursor failed: %v", err)
	}

	got, err := database.GetSyncCursor(remote)
	if err != nil {
		t.Fatalf("GetSyncCursor failed: %v", err)
	}
	if !got.Equal(want) {
		t.Errorf("GetSyncCursor returned %v, want %v", got, want)
	}
}

func TestSyncCursorOverwrite(t *testing.T) {
	tempDir := t.TempDir()
	oldHomeEnv := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHomeEnv)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	remote := "https://github.com/owner/project"
	t1 := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	t2 := time.Date(2026, 6, 15, 0, 0, 0, 0, time.UTC)

	// Write initial cursor
	if err := database.SetSyncCursor(remote, t1); err != nil {
		t.Fatalf("first SetSyncCursor failed: %v", err)
	}

	// Overwrite with newer time
	if err := database.SetSyncCursor(remote, t2); err != nil {
		t.Fatalf("second SetSyncCursor failed: %v", err)
	}

	// Verify the value was overwritten
	got, err := database.GetSyncCursor(remote)
	if err != nil {
		t.Fatalf("GetSyncCursor failed: %v", err)
	}
	if !got.Equal(t2) {
		t.Errorf("after overwrite, got %v, want %v", got, t2)
	}
}

func TestSyncCursorIndependentRemotes(t *testing.T) {
	tempDir := t.TempDir()
	oldHomeEnv := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHomeEnv)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	remoteA := "https://github.com/owner/project-a"
	remoteB := "https://github.com/owner/project-b"
	tA := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	tB := time.Date(2026, 4, 1, 0, 0, 0, 0, time.UTC)

	// Set both
	if err := database.SetSyncCursor(remoteA, tA); err != nil {
		t.Fatalf("SetSyncCursor remoteA failed: %v", err)
	}
	if err := database.SetSyncCursor(remoteB, tB); err != nil {
		t.Fatalf("SetSyncCursor remoteB failed: %v", err)
	}

	// Read each back independently
	gotA, err := database.GetSyncCursor(remoteA)
	if err != nil {
		t.Fatalf("GetSyncCursor remoteA failed: %v", err)
	}
	if !gotA.Equal(tA) {
		t.Errorf("remoteA got %v, want %v", gotA, tA)
	}

	gotB, err := database.GetSyncCursor(remoteB)
	if err != nil {
		t.Fatalf("GetSyncCursor remoteB failed: %v", err)
	}
	if !gotB.Equal(tB) {
		t.Errorf("remoteB got %v, want %v", gotB, tB)
	}

	// Verify they don't interfere — remoteB's cursor should not affect remoteA
	gotA2, err := database.GetSyncCursor(remoteA)
	if err != nil {
		t.Fatalf("GetSyncCursor remoteA (re-read) failed: %v", err)
	}
	if !gotA2.Equal(tA) {
		t.Errorf("remoteA after writing remoteB changed: got %v, want %v", gotA2, tA)
	}
}
