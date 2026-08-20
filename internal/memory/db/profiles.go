package db

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"
)

// Profile represents an agent or human identity with an assigned role.
type Profile struct {
	ID          string         `json:"id"`
	Name        string         `json:"name"`
	Type        string         `json:"type"`
	Role        string         `json:"role"`
	Description string         `json:"description"`
	Metadata    map[string]any `json:"metadata"`
	CreatedAt   time.Time      `json:"created_at"`
	UpdatedAt   time.Time      `json:"updated_at"`
}

// SaveProfile inserts or updates a profile.
func (db *DB) SaveProfile(p *Profile) error {
	metaJSON, err := json.Marshal(p.Metadata)
	if err != nil {
		return fmt.Errorf("marshal profile metadata: %w", err)
	}

	now := time.Now().UTC()
	if p.CreatedAt.IsZero() {
		p.CreatedAt = now
	}
	if p.UpdatedAt.IsZero() {
		p.UpdatedAt = now
	}

	query := `INSERT INTO profiles (id, name, type, role, description, metadata, created_at, updated_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			name=excluded.name,
			type=excluded.type,
			role=excluded.role,
			description=excluded.description,
			metadata=excluded.metadata,
			updated_at=excluded.updated_at`

	_, err = db.conn.Exec(query, p.ID, p.Name, p.Type, p.Role, p.Description, string(metaJSON), p.CreatedAt, p.UpdatedAt)
	return err
}

// GetProfileByName retrieves a profile by its unique name (case-insensitive).
func (db *DB) GetProfileByName(name string) (*Profile, error) {
	query := `SELECT id, name, type, role, description, metadata, created_at, updated_at
		FROM profiles WHERE name = ? COLLATE NOCASE`

	var p Profile
	var metaStr string
	err := db.conn.QueryRow(query, name).Scan(
		&p.ID, &p.Name, &p.Type, &p.Role, &p.Description,
		&metaStr, &p.CreatedAt, &p.UpdatedAt,
	)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}

	if err := json.Unmarshal([]byte(metaStr), &p.Metadata); err != nil {
		return nil, err
	}
	return &p, nil
}

// ListProfiles returns all stored profiles.
func (db *DB) ListProfiles() ([]*Profile, error) {
	query := `SELECT id, name, type, role, description, metadata, created_at, updated_at
		FROM profiles ORDER BY name COLLATE NOCASE`

	rows, err := db.conn.Query(query)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var profiles []*Profile
	for rows.Next() {
		var p Profile
		var metaStr string
		if err := rows.Scan(
			&p.ID, &p.Name, &p.Type, &p.Role, &p.Description,
			&metaStr, &p.CreatedAt, &p.UpdatedAt,
		); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(metaStr), &p.Metadata); err != nil {
			return nil, err
		}
		profiles = append(profiles, &p)
	}
	return profiles, nil
}

// DeleteProfile removes a profile by name (case-insensitive).
func (db *DB) DeleteProfile(name string) error {
	_, err := db.conn.Exec("DELETE FROM profiles WHERE name = ? COLLATE NOCASE", name)
	return err
}
