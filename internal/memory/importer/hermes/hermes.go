package hermes

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	_ "modernc.org/sqlite"
)

// HermesImporter imports sessions from Hermes Agent.
type HermesImporter struct {
	customPath string
}

// schemaInfo holds discovered table and column names from the database.
type schemaInfo struct {
	sessionIDCol  string
	messagesTable string
	roleCol       string
	contentCol    string
	timestampCol  string
}

func NewHermesImporter(customPath string) *HermesImporter {
	return &HermesImporter{customPath: customPath}
}

func (h *HermesImporter) Name() string {
	return "hermes"
}

func (h *HermesImporter) IsTranscript() bool { return true }

func (h *HermesImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	dbPath := h.customPath
	if dbPath == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		dbPath = filepath.Join(home, ".hermes", "state.db")
	}

	if _, err := os.Stat(dbPath); os.IsNotExist(err) {
		return nil, nil
	}

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, fmt.Errorf("failed to open hermes database: %w", err)
	}
	defer func() { _ = db.Close() }()

	schema, err := h.discoverSchema(db)
	if err != nil {
		return nil, fmt.Errorf("failed to discover hermes schema: %w", err)
	}

	// Discover sessions: pick session IDs from the messages table directly
	// (works even without a separate sessions table).
	var sessions []importer.SessionRef

	if schema.messagesTable != "" && schema.sessionIDCol != "" {
		query := fmt.Sprintf(
			"SELECT DISTINCT %s FROM %s WHERE %s > ? ORDER BY %s",
			schema.sessionIDCol, schema.messagesTable, schema.timestampCol, schema.timestampCol,
		)
		rows, err := db.Query(query, since)
		if err != nil {
			return nil, fmt.Errorf("failed to query sessions: %w", err)
		}
		defer func() { _ = rows.Close() }()

		for rows.Next() {
			var sessionID string
			if err := rows.Scan(&sessionID); err != nil {
				continue
			}
			sessions = append(sessions, importer.SessionRef{
				Tool:       "hermes",
				SessionID:  sessionID,
				Path:       dbPath,
				ModifiedAt: time.Now(),
				Metadata:   map[string]string{},
			})
		}
	}

	return sessions, nil
}

func (h *HermesImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	db, err := sql.Open("sqlite", ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to open hermes database: %w", err)
	}
	defer func() { _ = db.Close() }()

	schema, err := h.discoverSchema(db)
	if err != nil {
		return nil, fmt.Errorf("failed to discover hermes schema: %w", err)
	}

	if schema.messagesTable == "" {
		return nil, fmt.Errorf("no messages table found in hermes database")
	}

	query := fmt.Sprintf(
		"SELECT %s, %s, %s FROM %s WHERE %s = ? ORDER BY %s",
		schema.roleCol, schema.contentCol, schema.timestampCol,
		schema.messagesTable, schema.sessionIDCol, schema.timestampCol,
	)
	rows, err := db.Query(query, ref.SessionID)
	if err != nil {
		return nil, fmt.Errorf("failed to query messages: %w", err)
	}
	defer func() { _ = rows.Close() }()

	var facts []importer.ImportedFact

	for rows.Next() {
		var role, content string
		var timestamp time.Time
		if err := rows.Scan(&role, &content, &timestamp); err != nil {
			continue
		}
		if role == "assistant" && len(content) > 50 {
			facts = append(facts, importer.ImportedFact{
				Content:   content,
				Source:    "hermes",
				SessionID: ref.SessionID,
				Timestamp: timestamp,
				Metadata:  map[string]string{},
			})
		}
	}

	return facts, nil
}

// discoverSchema inspects the database to find tables and columns for sessions
// and messages. It tries common naming patterns.
func (h *HermesImporter) discoverSchema(db *sql.DB) (*schemaInfo, error) {
	tables, err := h.listTables(db)
	if err != nil {
		return nil, err
	}

	schema := &schemaInfo{}

	// Find the messages/turns table.
	for _, table := range tables {
		lower := strings.ToLower(table)
		if strings.Contains(lower, "message") || strings.Contains(lower, "turn") || strings.Contains(lower, "chat") {
			schema.messagesTable = table
			break
		}
	}

	if schema.messagesTable == "" && len(tables) > 0 {
		// Fallback: pick the first table that isn't sqlite_sequence or similar.
		for _, table := range tables {
			if table != "sqlite_sequence" && table != "sqlite_master" {
				schema.messagesTable = table
				break
			}
		}
	}

	if schema.messagesTable == "" {
		return schema, nil
	}

	// Discover columns in the messages table.
	columns, err := h.listColumns(db, schema.messagesTable)
	if err != nil {
		return nil, err
	}

	// Map columns to roles.
	for _, col := range columns {
		lower := strings.ToLower(col)
		if strings.Contains(lower, "session") || strings.Contains(lower, "conversation") || strings.Contains(lower, "thread") {
			if schema.sessionIDCol == "" {
				schema.sessionIDCol = col
			}
		}
		if lower == "role" || strings.Contains(lower, "role") || lower == "sender" {
			schema.roleCol = col
		}
		if lower == "content" || lower == "text" || lower == "body" || lower == "message" {
			schema.contentCol = col
		}
		if lower == "timestamp" || lower == "created_at" || lower == "time" || lower == "ts" || lower == "created" {
			schema.timestampCol = col
		}
	}

	// Sensible defaults for missing columns.
	if schema.roleCol == "" {
		schema.roleCol = "role"
	}
	if schema.contentCol == "" {
		schema.contentCol = "content"
	}
	if schema.timestampCol == "" {
		schema.timestampCol = "timestamp"
	}
	if schema.sessionIDCol == "" {
		schema.sessionIDCol = "session_id"
	}

	return schema, nil
}

// listTables returns all user table names from the database.
func (h *HermesImporter) listTables(db *sql.DB) ([]string, error) {
	rows, err := db.Query("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var tables []string
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			continue
		}
		tables = append(tables, name)
	}
	return tables, nil
}

// listColumns returns column names for a given table.
func (h *HermesImporter) listColumns(db *sql.DB, table string) ([]string, error) {
	rows, err := db.Query(fmt.Sprintf("PRAGMA table_info(%s)", table))
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var columns []string
	for rows.Next() {
		var (
			cid     int
			name    string
			ctype   string
			notnull int
			dflt    sql.NullString
			pk      int
		)
		if err := rows.Scan(&cid, &name, &ctype, &notnull, &dflt, &pk); err != nil {
			continue
		}
		columns = append(columns, name)
	}
	return columns, nil
}
