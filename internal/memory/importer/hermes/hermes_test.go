package hermes

import (
	"database/sql"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	_ "modernc.org/sqlite"
)

func TestNewHermesImporter(t *testing.T) {
	imp := NewHermesImporter("/tmp/hermes")

	if imp.Name() != "hermes" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "hermes")
	}
}

func TestNewHermesImporterDefaults(t *testing.T) {
	imp := NewHermesImporter("")

	if imp.customPath != "" {
		t.Errorf("customPath = %q, want empty", imp.customPath)
	}
}

// createTestDB creates a temporary SQLite database mimicking the hermes state.db schema.
func createTestDB(t *testing.T) string {
	t.Helper()
	tmpDir := t.TempDir()
	dbPath := filepath.Join(tmpDir, "state.db")

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		t.Fatalf("failed to open temp db: %v", err)
	}
	defer func() { _ = db.Close() }()

	// Create tables that match a realistic hermes state.db schema.
	_, err = db.Exec(`
		CREATE TABLE messages (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			session_id TEXT NOT NULL,
			role TEXT NOT NULL,
			content TEXT NOT NULL,
			timestamp DATETIME NOT NULL
		)
	`)
	if err != nil {
		t.Fatalf("failed to create messages table: %v", err)
	}

	// Insert test data.
	now := time.Date(2026, 6, 20, 10, 0, 0, 0, time.UTC)
	_, err = db.Exec(`INSERT INTO messages (session_id, role, content, timestamp) VALUES (?, ?, ?, ?)`,
		"sess-001", "user", "Write a binary search in Go", now)
	if err != nil {
		t.Fatal(err)
	}

	_, err = db.Exec(`INSERT INTO messages (session_id, role, content, timestamp) VALUES (?, ?, ?, ?)`,
		"sess-001", "assistant", "Here is a binary search implementation in Go that searches a sorted slice and returns the index of the target element, or -1 if not found.", now.Add(5*time.Second))
	if err != nil {
		t.Fatal(err)
	}

	_, err = db.Exec(`INSERT INTO messages (session_id, role, content, timestamp) VALUES (?, ?, ?, ?)`,
		"sess-001", "user", "Thanks", now.Add(10*time.Second))
	if err != nil {
		t.Fatal(err)
	}

	// Second session.
	_, err = db.Exec(`INSERT INTO messages (session_id, role, content, timestamp) VALUES (?, ?, ?, ?)`,
		"sess-002", "assistant", "Short", now.Add(15*time.Second))
	if err != nil {
		t.Fatal(err)
	}

	return dbPath
}

func TestDiscoverSessionsFindsSessions(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewHermesImporter(dbPath)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	// Verify session IDs are present.
	ids := make(map[string]bool)
	for _, s := range sessions {
		if s.Tool != "hermes" {
			t.Errorf("Tool = %q, want %q", s.Tool, "hermes")
		}
		ids[s.SessionID] = true
	}
	if !ids["sess-001"] || !ids["sess-002"] {
		t.Errorf("expected sess-001 and sess-002, got %v", ids)
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	dbPath := createTestDB(t)

	future := time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC)
	imp := NewHermesImporter(dbPath)
	sessions, err := imp.DiscoverSessions(future)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions (all older than since), got %d", len(sessions))
	}
}

func TestDiscoverSessionsMissingDB(t *testing.T) {
	imp := NewHermesImporter("/nonexistent/path/state.db")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should not error on missing db: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions when db missing, got %d", len(sessions))
	}
}

func TestImportSessionParsesMessages(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewHermesImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "hermes",
		SessionID:  "sess-001",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (assistant msg >50 chars), got %d", len(facts))
	}

	f := facts[0]
	if f.Source != "hermes" {
		t.Errorf("Source = %q, want %q", f.Source, "hermes")
	}
	if f.SessionID != "sess-001" {
		t.Errorf("SessionID = %q, want %q", f.SessionID, "sess-001")
	}
}

func TestImportSessionSkipsShortMessages(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewHermesImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "hermes",
		SessionID:  "sess-002",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	// sess-002 only has a "Short" message from assistant, should be skipped.
	if len(facts) != 0 {
		t.Fatalf("expected 0 facts (short msg), got %d", len(facts))
	}
}

func TestImportSessionNonexistentSession(t *testing.T) {
	dbPath := createTestDB(t)

	imp := NewHermesImporter(dbPath)
	ref := importer.SessionRef{
		Tool:       "hermes",
		SessionID:  "sess-nonexistent",
		Path:       dbPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 0 {
		t.Fatalf("expected 0 facts for nonexistent session, got %d", len(facts))
	}
}
