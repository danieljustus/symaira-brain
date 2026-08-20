package db

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"
)

// ConsolidationRun journals a single consolidation run for undo support.
type ConsolidationRun struct {
	ID                string    `json:"id"`
	RunAt             time.Time `json:"run_at"`
	Status            string    `json:"status"` // "completed" or "undone"
	SummaryJSON       string    `json:"summary_json"`
	TotalArchived     int       `json:"total_archived"`
	TotalConsolidated int       `json:"total_consolidated"`
}

// RunSummary is the JSON payload stored in summary_json; it captures
// exactly what the undo operation needs to reverse.
type RunSummary struct {
	NewMemoryIDs  []string          `json:"new_memory_ids"`
	ArchivedIDs   []string          `json:"archived_ids"`
	ReplacedToNew map[string]string `json:"replaced_to_new"` // oldID → newID for evidence reparenting
}

// SaveConsolidationRun persists a completed run to the journal.
func (db *DB) SaveConsolidationRun(run *ConsolidationRun) error {
	_, err := db.conn.Exec(
		`INSERT INTO consolidation_runs (id, run_at, status, summary_json, total_archived, total_consolidated)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		run.ID, run.RunAt.UTC(), run.Status, run.SummaryJSON, run.TotalArchived, run.TotalConsolidated,
	)
	return err
}

// GetLastCompletedRun returns the most recent completed (not undone) run, or
// nil if none exists.
func (db *DB) GetLastCompletedRun() (*ConsolidationRun, error) {
	var run ConsolidationRun
	var runAt string
	err := db.conn.QueryRow(
		`SELECT id, run_at, status, summary_json, total_archived, total_consolidated
		 FROM consolidation_runs WHERE status = 'completed'
		 ORDER BY run_at DESC LIMIT 1`,
	).Scan(&run.ID, &runAt, &run.Status, &run.SummaryJSON, &run.TotalArchived, &run.TotalConsolidated)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	run.RunAt, err = parseConsolidationRunTime(runAt)
	if err != nil {
		return nil, fmt.Errorf("parse run_at %q: %w", runAt, err)
	}
	return &run, nil
}

// parseConsolidationRunTime parses run_at as stored by the SQLite driver.
// SaveConsolidationRun writes time.Time values, which the modernc driver
// stores as Go time.String() text ("2006-01-02 15:04:05.999999999 -0700 MST").
// Older databases may contain the two previously used layouts, so keep those
// as fallbacks.
func parseConsolidationRunTime(s string) (time.Time, error) {
	layouts := []string{
		"2006-01-02 15:04:05.999999999 -0700 MST",
		"2006-01-02 15:04:05 -0700 MST",
		"2006-01-02 15:04:05-07:00",
		"2006-01-02 15:04:05",
	}
	var lastErr error
	for _, layout := range layouts {
		t, err := time.Parse(layout, s)
		if err == nil {
			return t, nil
		}
		lastErr = err
	}
	return time.Time{}, lastErr
}

// MarkRunUndone sets a run's status to 'undone'.
func (db *DB) MarkRunUndone(runID string) error {
	_, err := db.conn.Exec(`UPDATE consolidation_runs SET status = 'undone' WHERE id = ?`, runID)
	return err
}

// MarkRunUndoneTx sets a run's status to 'undone' inside the caller's
// transaction. UndoLastConsolidation must use this variant: it runs inside
// an open transaction, and a separate connection-level Exec would deadlock
// on the single-connection SQLite driver.
func (db *DB) MarkRunUndoneTx(tx *sql.Tx, runID string) error {
	_, err := tx.Exec(`UPDATE consolidation_runs SET status = 'undone' WHERE id = ?`, runID)
	return err
}

// ParseRunSummary deserialises the summary JSON stored in a run.
func ParseRunSummary(s string) (*RunSummary, error) {
	var rs RunSummary
	if err := json.Unmarshal([]byte(s), &rs); err != nil {
		return nil, err
	}
	return &rs, nil
}

// MarshalRunSummary serialises a RunSummary to JSON.
func MarshalRunSummary(rs *RunSummary) (string, error) {
	b, err := json.Marshal(rs)
	if err != nil {
		return "", err
	}
	return string(b), nil
}
