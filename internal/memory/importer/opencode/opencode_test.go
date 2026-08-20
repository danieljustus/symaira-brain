package opencode

import (
	"database/sql"
	"fmt"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	_ "modernc.org/sqlite"
)

func TestNewOpenCodeImporter(t *testing.T) {
	imp := NewOpenCodeImporter("/tmp/opencode.db")

	if imp.Name() != "opencode" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "opencode")
	}
	if imp.Category() != "code" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "code")
	}
	if imp.PrivacyLevel() != importer.PrivacyInternal {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), importer.PrivacyInternal)
	}
	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
	if !imp.IsTranscript() {
		t.Error("IsTranscript() = false, want true")
	}
}

func TestNewOpenCodeImporterDefaults(t *testing.T) {
	imp := NewOpenCodeImporter("")

	if imp.customPath != "" {
		t.Errorf("customPath = %q, want empty", imp.customPath)
	}
}

// createTestDB creates a temporary SQLite database mimicking the OpenCode schema.
func createTestDB(t *testing.T) string {
	t.Helper()
	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "opencode.db")

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		t.Fatalf("failed to open temp db: %v", err)
	}
	defer func() { _ = db.Close() }()

	// Create tables matching the real OpenCode schema.
	_, err = db.Exec(`
		CREATE TABLE project (
			id TEXT PRIMARY KEY,
			worktree TEXT NOT NULL,
			vcs TEXT,
			name TEXT,
			icon_url TEXT,
			icon_color TEXT,
			time_created INTEGER NOT NULL,
			time_updated INTEGER NOT NULL,
			time_initialized INTEGER,
			sandboxes TEXT NOT NULL,
			commands TEXT,
			icon_url_override TEXT
		)
	`)
	if err != nil {
		t.Fatalf("failed to create project table: %v", err)
	}

	_, err = db.Exec(`
		CREATE TABLE session (
			id TEXT PRIMARY KEY,
			project_id TEXT NOT NULL,
			parent_id TEXT,
			slug TEXT NOT NULL,
			directory TEXT NOT NULL,
			title TEXT NOT NULL,
			version TEXT NOT NULL,
			share_url TEXT,
			summary_additions INTEGER,
			summary_deletions INTEGER,
			summary_files INTEGER,
			summary_diffs TEXT,
			revert TEXT,
			permission TEXT,
			time_created INTEGER NOT NULL,
			time_updated INTEGER NOT NULL,
			time_compacting INTEGER,
			time_archived INTEGER,
			workspace_id TEXT,
			path TEXT,
			agent TEXT,
			model TEXT,
			cost REAL DEFAULT 0 NOT NULL,
			tokens_input INTEGER DEFAULT 0 NOT NULL,
			tokens_output INTEGER DEFAULT 0 NOT NULL,
			tokens_reasoning INTEGER DEFAULT 0 NOT NULL,
			tokens_cache_read INTEGER DEFAULT 0 NOT NULL,
			tokens_cache_write INTEGER DEFAULT 0 NOT NULL,
			metadata TEXT
		)
	`)
	if err != nil {
		t.Fatalf("failed to create session table: %v", err)
	}

	_, err = db.Exec(`
		CREATE TABLE message (
			id TEXT PRIMARY KEY,
			session_id TEXT NOT NULL,
			time_created INTEGER NOT NULL,
			time_updated INTEGER NOT NULL,
			data TEXT NOT NULL
		)
	`)
	if err != nil {
		t.Fatalf("failed to create message table: %v", err)
	}

	_, err = db.Exec(`
		CREATE TABLE part (
			id TEXT PRIMARY KEY,
			message_id TEXT NOT NULL,
			session_id TEXT NOT NULL,
			time_created INTEGER NOT NULL,
			time_updated INTEGER NOT NULL,
			data TEXT NOT NULL
		)
	`)
	if err != nil {
		t.Fatalf("failed to create part table: %v", err)
	}

	// Insert test project.
	_, err = db.Exec(`INSERT INTO project (id, worktree, name, time_created, time_updated, sandboxes) VALUES (?, ?, ?, ?, ?, ?)`,
		"proj-001", "/Users/test/project", "test-project", 1782200000000, 1782294000000, "[]")
	if err != nil {
		t.Fatal(err)
	}

	// Insert test sessions.
	now := time.Date(2026, 6, 24, 10, 0, 0, 0, time.UTC)
	_, err = db.Exec(`INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, agent, model) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		"ses-001", "proj-001", "test-session", "/Users/test/project", "Implement feature X", "1.0.0",
		now.UnixMilli(), now.Add(5*time.Minute).UnixMilli(), "Sisyphus-Junior", `{"id":"mimo-v2.5","providerID":"opencode-go"}`)
	if err != nil {
		t.Fatal(err)
	}

	_, err = db.Exec(`INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, agent, model) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		"ses-002", "proj-001", "short-session", "/Users/test/project", "Short session", "1.0.0",
		now.UnixMilli(), now.Add(10*time.Minute).UnixMilli(), "Sisyphus-Junior", `{"id":"mimo-v2.5","providerID":"opencode-go"}`)
	if err != nil {
		t.Fatal(err)
	}

	// Insert messages for ses-001.
	// User message.
	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-001", "ses-001", now.UnixMilli(), now.UnixMilli(),
		`{"role":"user","time":{"created":`+formatMs(now)+`}}`)
	if err != nil {
		t.Fatal(err)
	}

	// Assistant message.
	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-002", "ses-001", now.Add(1*time.Second).UnixMilli(), now.Add(1*time.Second).UnixMilli(),
		`{"role":"assistant","time":{"created":`+formatMs(now.Add(1*time.Second))+`},"tokens":{"input":1000,"output":200},"modelID":"mimo-v2.5","providerID":"opencode-go"}`)
	if err != nil {
		t.Fatal(err)
	}

	// User message 2.
	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-003", "ses-001", now.Add(5*time.Second).UnixMilli(), now.Add(5*time.Second).UnixMilli(),
		`{"role":"user","time":{"created":`+formatMs(now.Add(5*time.Second))+`}}`)
	if err != nil {
		t.Fatal(err)
	}

	// Assistant message 2.
	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-004", "ses-001", now.Add(6*time.Second).UnixMilli(), now.Add(6*time.Second).UnixMilli(),
		`{"role":"assistant","time":{"created":`+formatMs(now.Add(6*time.Second))+`},"tokens":{"input":1200,"output":300},"modelID":"mimo-v2.5","providerID":"opencode-go"}`)
	if err != nil {
		t.Fatal(err)
	}

	// Insert text parts for messages.
	// User part for msg-001.
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-001", "msg-001", "ses-001", now.UnixMilli(), now.UnixMilli(),
		`{"type":"text","text":"Implement a new feature that adds dark mode support to the application settings panel."}`)
	if err != nil {
		t.Fatal(err)
	}

	// Assistant text part for msg-002 (long response).
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-002", "msg-002", "ses-001", now.Add(1*time.Second).UnixMilli(), now.Add(1*time.Second).UnixMilli(),
		`{"type":"text","text":"I'll implement the dark mode feature by adding a theme toggle in the settings panel. First, I'll create a ThemeProvider component that manages the current theme state and provides it to the application. Then I'll add CSS variables for light and dark themes, and wire up the toggle button to switch between them. The implementation will use system preferences as the default and persist the user's choice in localStorage."}`)
	if err != nil {
		t.Fatal(err)
	}

	// Tool part for msg-002 (should be ignored).
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-003", "msg-002", "ses-001", now.Add(1500*time.Millisecond).UnixMilli(), now.Add(1500*time.Millisecond).UnixMilli(),
		`{"type":"tool","tool":"bash","state":{"status":"completed"}}`)
	if err != nil {
		t.Fatal(err)
	}

	// User text part for msg-003.
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-004", "msg-003", "ses-001", now.Add(5*time.Second).UnixMilli(), now.Add(5*time.Second).UnixMilli(),
		`{"type":"text","text":"Great, now add tests for the theme toggle."}`)
	if err != nil {
		t.Fatal(err)
	}

	// Assistant text part for msg-004 (long response).
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-005", "msg-004", "ses-001", now.Add(6*time.Second).UnixMilli(), now.Add(6*time.Second).UnixMilli(),
		`{"type":"text","text":"I'll add comprehensive tests for the theme toggle component. The tests will cover rendering the toggle button, clicking to switch themes, verifying CSS class changes on the root element, and testing that the selected theme persists in localStorage. I'll also add tests for system preference detection."}`)
	if err != nil {
		t.Fatal(err)
	}

	// Short session (ses-002) with short messages.
	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-005", "ses-002", now.Add(10*time.Minute).UnixMilli(), now.Add(10*time.Minute).UnixMilli(),
		`{"role":"user","time":{"created":`+formatMs(now.Add(10*time.Minute))+`}}`)
	if err != nil {
		t.Fatal(err)
	}

	_, err = db.Exec(`INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?)`,
		"msg-006", "ses-002", now.Add(10*time.Minute+1*time.Second).UnixMilli(), now.Add(10*time.Minute+1*time.Second).UnixMilli(),
		`{"role":"assistant","time":{"created":`+formatMs(now.Add(10*time.Minute+1*time.Second))+`}}`)
	if err != nil {
		t.Fatal(err)
	}

	// Short text parts for ses-002.
	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-006", "msg-005", "ses-002", now.Add(10*time.Minute).UnixMilli(), now.Add(10*time.Minute).UnixMilli(),
		`{"type":"text","text":"Hello"}`)
	if err != nil {
		t.Fatal(err)
	}

	_, err = db.Exec(`INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?, ?, ?, ?, ?, ?)`,
		"prt-007", "msg-006", "ses-002", now.Add(10*time.Minute+1*time.Second).UnixMilli(), now.Add(10*time.Minute+1*time.Second).UnixMilli(),
		`{"type":"text","text":"Hi there!"}`)
	if err != nil {
		t.Fatal(err)
	}

	return dbPath
}

func formatMs(t time.Time) string {
	return fmt.Sprintf("%d", t.UnixMilli())
}

func TestDiscoverSessionsFindsSessions(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	ids := make(map[string]bool)
	for _, s := range sessions {
		if s.Tool != "opencode" {
			t.Errorf("Tool = %q, want %q", s.Tool, "opencode")
		}
		ids[s.SessionID] = true
	}
	if !ids["ses-001"] || !ids["ses-002"] {
		t.Errorf("expected ses-001 and ses-002, got %v", ids)
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	dbPath := createTestDB(t)

	// Future time: no sessions should match.
	future := time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC)
	imp := NewOpenCodeImporter(dbPath)
	sessions, err := imp.DiscoverSessions(future)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions (all older than since), got %d", len(sessions))
	}
}

func TestDiscoverSessionsMissingDB(t *testing.T) {
	imp := NewOpenCodeImporter("/nonexistent/path/opencode.db")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should not error on missing db: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions when db missing, got %d", len(sessions))
	}
}

func TestDiscoverSessionsMetadata(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	for _, s := range sessions {
		if s.Metadata["title"] == "" {
			t.Errorf("session %s missing title metadata", s.SessionID)
		}
		if s.Metadata["directory"] == "" {
			t.Errorf("session %s missing directory metadata", s.SessionID)
		}
		if s.Metadata["agent"] == "" {
			t.Errorf("session %s missing agent metadata", s.SessionID)
		}
	}
}

func TestImportSessionParsesMessages(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "opencode",
		SessionID:  "ses-001",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	// Should have 2 facts (2 assistant messages with long text parts).
	if len(facts) != 2 {
		t.Fatalf("expected 2 facts (assistant msgs with long text), got %d", len(facts))
	}

	for _, f := range facts {
		if f.Source != "opencode" {
			t.Errorf("Source = %q, want %q", f.Source, "opencode")
		}
		if f.SessionID != "ses-001" {
			t.Errorf("SessionID = %q, want %q", f.SessionID, "ses-001")
		}
		if f.Metadata["title"] != "Implement feature X" {
			t.Errorf("title = %q, want %q", f.Metadata["title"], "Implement feature X")
		}
		if f.Metadata["directory"] != "/Users/test/project" {
			t.Errorf("directory = %q, want %q", f.Metadata["directory"], "/Users/test/project")
		}
	}
}

func TestImportSessionSkipsShortMessages(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "opencode",
		SessionID:  "ses-002",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	// ses-002 only has short messages, should be skipped.
	if len(facts) != 0 {
		t.Fatalf("expected 0 facts (short msgs), got %d", len(facts))
	}
}

func TestImportSessionNonexistentSession(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "opencode",
		SessionID:  "ses-nonexistent",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for nonexistent session")
	}
}

func TestImportSessionMetadataEnrichment(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "opencode",
		SessionID:  "ses-001",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) == 0 {
		t.Fatal("expected at least one fact")
	}

	f := facts[0]
	if f.Metadata["model"] != "opencode-go/mimo-v2.5" {
		t.Errorf("model metadata = %q, want %q", f.Metadata["model"], "opencode-go/mimo-v2.5")
	}
	if f.Metadata["message_id"] == "" {
		t.Error("expected message_id metadata to be set")
	}
}

func TestImportSessionTimestamps(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewOpenCodeImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "opencode",
		SessionID:  "ses-001",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) == 0 {
		t.Fatal("expected at least one fact")
	}

	// First fact should have a timestamp from the first assistant message.
	if facts[0].Timestamp.IsZero() {
		t.Error("expected non-zero timestamp for first fact")
	}

	// Second fact should have a later timestamp.
	if len(facts) > 1 && !facts[1].Timestamp.After(facts[0].Timestamp) {
		t.Error("expected second fact timestamp to be after first")
	}
}

func TestDbPathDefault(t *testing.T) {
	imp := NewOpenCodeImporter("")
	path := imp.dbPath()

	// Should return a path ending with opencode.db under the expected directory.
	if filepath.Base(path) != "opencode.db" {
		t.Errorf("dbPath() = %q, expected to end with opencode.db", path)
	}
}

func TestDbPathCustom(t *testing.T) {
	imp := NewOpenCodeImporter("/custom/path/opencode.db")
	path := imp.dbPath()

	if path != "/custom/path/opencode.db" {
		t.Errorf("dbPath() = %q, want /custom/path/opencode.db", path)
	}
}
