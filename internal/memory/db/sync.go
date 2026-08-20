package db

import (
	"database/sql"
	"time"
)

// GetSyncCursor returns the last sync timestamp for a given remote URL.
// Returns zero time if no cursor has been stored yet.
func (db *DB) GetSyncCursor(remote string) (time.Time, error) {
	var t time.Time
	err := db.conn.QueryRow(
		"SELECT last_sync FROM sync_state WHERE remote = ?", remote,
	).Scan(&t)
	if err == sql.ErrNoRows {
		return time.Time{}, nil
	}
	if err != nil {
		return time.Time{}, err
	}
	return t, nil
}

// SetSyncCursor upserts the last sync timestamp for a given remote URL.
func (db *DB) SetSyncCursor(remote string, t time.Time) error {
	_, err := db.conn.Exec(
		`INSERT INTO sync_state (remote, last_sync) VALUES (?, ?)
		 ON CONFLICT(remote) DO UPDATE SET last_sync = excluded.last_sync`,
		remote, t,
	)
	return err
}
