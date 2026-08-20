package db

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"
)

// Rule represents a stored procedural behavioral instruction.
type Rule struct {
	ID        string            `json:"id"`
	Content   string            `json:"content"`
	Scope     string            `json:"scope"`
	Metadata  map[string]string `json:"metadata"`
	CreatedAt time.Time         `json:"created_at"`
	UpdatedAt time.Time         `json:"updated_at"`
	CreatedBy string            `json:"created_by,omitempty"`
	UpdatedBy string            `json:"updated_by,omitempty"`
}

// SaveRule inserts or updates a procedural memory rule.
func (db *DB) SaveRule(r *Rule) error {
	metadataJSON, err := json.Marshal(r.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %w", err)
	}

	query := `INSERT INTO rules (id, content, scope, metadata, created_at, updated_at, created_by, updated_by)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			content=excluded.content,
			scope=excluded.scope,
			metadata=excluded.metadata,
			updated_at=excluded.updated_at,
			updated_by=excluded.updated_by`

	now := time.Now().UTC()
	if r.CreatedAt.IsZero() {
		r.CreatedAt = now
	}
	if r.UpdatedAt.IsZero() {
		r.UpdatedAt = now
	}

	_, err = db.conn.Exec(query, r.ID, r.Content, r.Scope, string(metadataJSON), r.CreatedAt, r.UpdatedAt, r.CreatedBy, r.UpdatedBy)
	return err
}

// DeleteRule removes a procedural rule by ID.
func (db *DB) DeleteRule(id string) error {
	_, err := db.conn.Exec("DELETE FROM rules WHERE id = ?", id)
	return err
}

// ListRules retrieves stored rules, optionally filtered by scope.
func (db *DB) ListRules(scope string) ([]*Rule, error) {
	var query string
	var rows *sql.Rows
	var err error

	if scope != "" {
		query = "SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by FROM rules WHERE scope = ? ORDER BY created_at DESC"
		rows, err = db.conn.Query(query, scope)
	} else {
		query = "SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by FROM rules ORDER BY created_at DESC"
		rows, err = db.conn.Query(query)
	}

	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var rules []*Rule
	for rows.Next() {
		var r Rule
		var metaStr string
		if err := rows.Scan(&r.ID, &r.Content, &r.Scope, &metaStr, &r.CreatedAt, &r.UpdatedAt, &r.CreatedBy, &r.UpdatedBy); err != nil {
			return nil, err
		}

		if err := json.Unmarshal([]byte(metaStr), &r.Metadata); err != nil {
			return nil, err
		}

		rules = append(rules, &r)
	}

	return rules, nil
}
