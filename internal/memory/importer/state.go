package importer

import (
	"database/sql"
	"time"
)

// ImportState tracks which sessions have been imported to prevent duplicates.
type ImportState struct {
	conn *sql.DB
}

// NewImportState creates a new import state tracker.
func NewImportState(conn *sql.DB) *ImportState {
	return &ImportState{conn: conn}
}

// IsImported checks if a session has already been imported.
func (s *ImportState) IsImported(tool, sessionID string) (bool, error) {
	var count int
	err := s.conn.QueryRow(
		"SELECT COUNT(*) FROM import_state WHERE tool = ? AND session_id = ?",
		tool, sessionID,
	).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

// MarkImported records that a session has been imported.
func (s *ImportState) MarkImported(tool, sessionID string, memoryCount int) error {
	_, err := s.conn.Exec(
		`INSERT INTO import_state (tool, session_id, imported_at, memory_count)
		 VALUES (?, ?, ?, ?)
		 ON CONFLICT(tool, session_id) DO UPDATE SET
			imported_at = excluded.imported_at,
			memory_count = excluded.memory_count`,
		tool, sessionID, time.Now().UTC(), memoryCount,
	)
	return err
}

// GetLastImportTime returns the most recent import time for a tool.
func (s *ImportState) GetLastImportTime(tool string) (time.Time, error) {
	var t sql.NullTime
	err := s.conn.QueryRow(
		"SELECT MAX(imported_at) FROM import_state WHERE tool = ?", tool,
	).Scan(&t)
	if err != nil {
		return time.Time{}, err
	}
	if !t.Valid {
		return time.Time{}, nil
	}
	return t.Time, nil
}

// GetImportCount returns the total number of imported sessions for a tool.
func (s *ImportState) GetImportCount(tool string) (int, error) {
	var count int
	err := s.conn.QueryRow(
		"SELECT COUNT(*) FROM import_state WHERE tool = ?", tool,
	).Scan(&count)
	if err != nil {
		return 0, err
	}
	return count, nil
}
