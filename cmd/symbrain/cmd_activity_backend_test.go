//nolint:gosec // This file writes malformed test configuration and database values only.
package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdActivityBackendErrorEdges(t *testing.T) {
	home := sandboxHome(t)
	writeActivityReadProfile(t, home, "activity", true)
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)

	configHome := t.TempDir()
	if err := os.MkdirAll(filepath.Join(configHome, "symbrain"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configHome, "symbrain", "config.toml"), []byte("[broken"), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("XDG_CONFIG_HOME", configHome)
	if _, database, err := openActivityStore(""); err != nil {
		t.Fatalf("config fallback: %v", err)
	} else if err := database.Close(); err != nil {
		t.Fatal(err)
	}
	// Corekit v0.16.2 honors XDG_CONFIG_HOME. Restore the sandbox config
	// root before the activity command reads the profile created above.
	t.Setenv("XDG_CONFIG_HOME", "")

	dbPath := newCLIActivityTestDB(t, base)
	mutate := func(sql string, args ...any) {
		t.Helper()
		cfg := config.Defaults()
		cfg.Database.Path = dbPath
		database, err := db.Open(cfg)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := database.Conn().Exec(sql, args...); err != nil {
			_ = database.Close()
			t.Fatal(err)
		}
		if err := database.Close(); err != nil {
			t.Fatal(err)
		}
	}
	mutate("UPDATE activity_segments SET applications = ? WHERE id = ?", "not-json", "cli-segment")
	var searchOut, searchErr bytes.Buffer
	if code := run([]string{"activity", "search", "--profile", "activity", "--from", base.Format(time.RFC3339), "--to", base.Add(time.Hour).Format(time.RFC3339), "--limit", "1", "--max-tokens", "10", "--db", dbPath, "editor"}, &searchOut, &searchErr); code != exitcodes.ExitGeneric {
		t.Fatalf("search backend error = %d, stderr=%q", code, searchErr.String())
	}
	var getOut, getErr bytes.Buffer
	if code := run([]string{"activity", "get", "--profile", "activity", "--max-tokens", "10", "--db", dbPath, "cli-segment"}, &getOut, &getErr); code != exitcodes.ExitGeneric {
		t.Fatalf("get backend error = %d, stderr=%q", code, getErr.String())
	}
	mutate("UPDATE activity_segments SET applications = ?, started_at = ?, ended_at = ? WHERE id = ?", "[]", "not-a-time", "not-a-time", "cli-segment")
	var statusOut, statusErr bytes.Buffer
	if code := run([]string{"activity", "status", "--profile", "activity", "--max-tokens", "10", "--db", dbPath}, &statusOut, &statusErr); code != exitcodes.ExitGeneric {
		t.Fatalf("status backend error = %d, stderr=%q", code, statusErr.String())
	}
	var validationOut, validationErr bytes.Buffer
	longQuery := string(make([]rune, activity.MaxQueryLength+1))
	if code := run([]string{"activity", "search", "--profile", "activity", "--from", base.Format(time.RFC3339), "--to", base.Add(time.Hour).Format(time.RFC3339), "--limit", "1", "--max-tokens", "10", longQuery}, &validationOut, &validationErr); code != exitcodes.ExitNoInput {
		t.Fatalf("validation error = %d, stderr=%q", code, validationErr.String())
	}
}
