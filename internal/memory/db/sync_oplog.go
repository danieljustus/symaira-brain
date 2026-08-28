package db

import (
	"database/sql"
	"time"
)

// DeletedMemory is a tombstone entry describing a memory deleted locally.
type DeletedMemory struct {
	ID        string    `json:"id"`
	DeletedAt time.Time `json:"deleted_at"`
}

// GetDeletedSince returns tombstones for memories whose latest oplog entry is
// a delete recorded strictly after since. Memories re-created after their
// deletion are not reported.
func (db *DB) GetDeletedSince(since time.Time) ([]DeletedMemory, error) {
	rows, err := db.conn.Query(`
		SELECT o.memory_id, o.ts FROM sync_oplog o
		WHERE o.op = 'delete'
		  AND o.ts > ?
		  AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)`,
		since)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var deleted []DeletedMemory
	for rows.Next() {
		var d DeletedMemory
		if err := rows.Scan(&d.ID, &d.DeletedAt); err != nil {
			return nil, err
		}
		deleted = append(deleted, d)
	}
	return deleted, rows.Err()
}

// GetDeletedSinceCursorID returns tombstones updated after since, with
// keyset pagination. When lastID is empty it starts from since. After each
// page the caller should pass the last row's DeletedAt and ID as the new
// since and lastID so rows with equal timestamps are not lost.
func (db *DB) GetDeletedSinceCursorID(since time.Time, lastID string, limit int) ([]DeletedMemory, error) {
	if limit <= 0 {
		limit = 50000
	}
	var query string
	var args []any
	if lastID == "" {
		query = `SELECT o.memory_id, o.ts FROM sync_oplog o
			WHERE o.op = 'delete'
			  AND o.ts > ?
			  AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)
			ORDER BY o.ts ASC, o.memory_id ASC
			LIMIT ?`
		args = []any{since, limit}
	} else {
		query = `SELECT o.memory_id, o.ts FROM sync_oplog o
			WHERE (o.ts > ? OR (o.ts = ? AND o.memory_id > ?))
			  AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id)
			ORDER BY o.ts ASC, o.memory_id ASC
			LIMIT ?`
		args = []any{since, since, lastID, limit}
	}

	rows, err := db.conn.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var deleted []DeletedMemory
	for rows.Next() {
		var d DeletedMemory
		if err := rows.Scan(&d.ID, &d.DeletedAt); err != nil {
			return nil, err
		}
		deleted = append(deleted, d)
	}
	return deleted, rows.Err()
}

// GetTombstone reports whether the latest oplog entry for id is a delete,
// returning its timestamp when it is.
func (db *DB) GetTombstone(id string) (time.Time, bool, error) {
	var op string
	var ts time.Time
	err := db.conn.QueryRow(`
		SELECT op, ts FROM sync_oplog
		WHERE memory_id = ? ORDER BY event_id DESC LIMIT 1`, id).Scan(&op, &ts)
	if err == sql.ErrNoRows {
		return time.Time{}, false, nil
	}
	if err != nil {
		return time.Time{}, false, err
	}
	return ts, op == "delete", nil
}

// ApplyRemoteDelete removes the local memory when the remote tombstone is at
// least as new as the local row (last-writer-wins). Returns true when a row
// was deleted.
func (db *DB) ApplyRemoteDelete(id string, deletedAt time.Time) (bool, error) {
	var localUpdated time.Time
	err := db.conn.QueryRow("SELECT updated_at FROM memories WHERE id = ?", id).Scan(&localUpdated)
	if err == sql.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if localUpdated.After(deletedAt) {
		return false, nil
	}
	if err := db.DeleteMemory(id); err != nil {
		return false, err
	}
	return true, nil
}

// SyncUpsertMemoryIfNewer is the tombstone-aware variant of
// UpsertMemoryIfNewer for sync ingestion: a pulled row is skipped when a
// local delete tombstone is newer than the incoming row, so deleted memories
// are not resurrected by a peer that has not seen the delete yet.
func (db *DB) SyncUpsertMemoryIfNewer(m *Memory) (bool, error) {
	ts, isTombstone, err := db.GetTombstone(m.ID)
	if err != nil {
		return false, err
	}
	if isTombstone && !m.UpdatedAt.After(ts) {
		return false, nil
	}
	return db.UpsertMemoryIfNewer(m)
}

// RelayBlob is an opaque, client-side-encrypted sync payload stored on a
// relay instance. The relay never sees plaintext memory content — only the
// memory id, the operation timestamp and the ciphertext.
type RelayBlob struct {
	ID        string    `json:"id"`
	UpdatedAt time.Time `json:"updated_at"`
	Blob      []byte    `json:"blob"`
}

// StoreRelayBlob upserts a relay blob using last-writer-wins on updated_at.
// Returns true when the blob was stored or overwritten.
func (db *DB) StoreRelayBlob(b RelayBlob) (bool, error) {
	var existing time.Time
	err := db.conn.QueryRow("SELECT updated_at FROM sync_relay WHERE id = ?", b.ID).Scan(&existing)
	if err != nil && err != sql.ErrNoRows {
		return false, err
	}
	if err == nil && !b.UpdatedAt.After(existing) {
		return false, nil
	}
	_, err = db.conn.Exec(
		`INSERT INTO sync_relay (id, updated_at, blob) VALUES (?, ?, ?)
		 ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at, blob = excluded.blob`,
		b.ID, b.UpdatedAt, b.Blob)
	if err != nil {
		return false, err
	}
	return true, nil
}

// GetRelayBlobsSince returns relay blobs updated strictly after since,
// ordered by updated_at ascending.
func (db *DB) GetRelayBlobsSince(since time.Time, limit int) ([]RelayBlob, error) {
	if limit <= 0 {
		limit = 50000
	}
	rows, err := db.conn.Query(
		"SELECT id, updated_at, blob FROM sync_relay WHERE updated_at > ? ORDER BY updated_at ASC LIMIT ?",
		since, limit)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var blobs []RelayBlob
	for rows.Next() {
		var b RelayBlob
		if err := rows.Scan(&b.ID, &b.UpdatedAt, &b.Blob); err != nil {
			return nil, err
		}
		blobs = append(blobs, b)
	}
	return blobs, rows.Err()
}
