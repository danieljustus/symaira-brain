package db

import (
	"database/sql"
	"time"
)

// Session represents a compressed summary of a chat session.
type Session struct {
	ID        string    `json:"id"`
	Summary   string    `json:"summary"`
	UpdatedAt time.Time `json:"updated_at"`
}

// SaveSessionSummary inserts or updates a session summary.
func (db *DB) SaveSessionSummary(id, summary string) error {
	query := `INSERT INTO sessions (id, summary, updated_at)
		VALUES (?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			summary=excluded.summary,
			updated_at=excluded.updated_at`

	_, err := db.conn.Exec(query, id, summary, time.Now())
	return err
}

// GetSessionSummary retrieves a session summary by ID.
func (db *DB) GetSessionSummary(id string) (string, error) {
	var summary string
	err := db.conn.QueryRow("SELECT summary FROM sessions WHERE id = ?", id).Scan(&summary)
	if err == sql.ErrNoRows {
		return "", nil
	}
	return summary, err
}
