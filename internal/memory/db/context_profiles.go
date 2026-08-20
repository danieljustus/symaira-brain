package db

import (
	"database/sql"
	"fmt"
	"time"
)

// ContextProfile represents a named, ordered collection of scope links
// for memory retrieval. This is distinct from the access-control Profile
// (which controls RBAC) — a ContextProfile controls which scopes to
// search and in what precedence order.
type ContextProfile struct {
	ID          string    `json:"id"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	BaseScope   string    `json:"base_scope"` // fallback scope when no links exist
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// ContextProfileLink defines a single scope entry within a context profile.
// Links are resolved in ascending order of precedence_order; ties are
// broken by the link's auto-increment id (created_at as secondary tiebreak).
//
// If parent_profile_id is set, the link pulls in all resolved scopes from
// that parent profile before continuing with subsequent links in this profile.
// filter_key/filter_value optionally restrict the link to memories whose
// metadata[filter_key] == filter_value during resolution.
type ContextProfileLink struct {
	ID              int64     `json:"id"`
	ProfileID       string    `json:"profile_id"`
	ParentProfileID *string   `json:"parent_profile_id,omitempty"`
	Scope           string    `json:"scope"`            // one of: global, project, agent, user, session
	FilterKey       string    `json:"filter_key"`       // optional metadata key to filter by
	FilterValue     string    `json:"filter_value"`     // required when filter_key is set
	PrecedenceOrder int       `json:"precedence_order"` // lower = searched first
	CreatedAt       time.Time `json:"created_at"`
}

// ResolvedScope is a fully resolved scope entry produced by ResolveContextProfile.
// It carries the effective scope name, the originating profile name, and any
// optional metadata filter so callers can tag search results with provenance.
type ResolvedScope struct {
	Scope       string `json:"scope"`
	Profile     string `json:"profile"`      // which profile this scope came from
	FilterKey   string `json:"filter_key"`   // optional metadata filter key
	FilterValue string `json:"filter_value"` // optional metadata filter value
}

const (
	// DefaultMaxDepth is the default recursion limit for profile inheritance.
	DefaultMaxDepth = 8
)

// SaveContextProfile inserts or updates a context profile.
func (db *DB) SaveContextProfile(cp *ContextProfile) error {
	now := time.Now().UTC()
	if cp.CreatedAt.IsZero() {
		cp.CreatedAt = now
	}
	cp.UpdatedAt = now

	_, err := db.conn.Exec(
		`INSERT INTO context_profiles (id, name, description, base_scope, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?)
		 ON CONFLICT(id) DO UPDATE SET
			name=excluded.name,
			description=excluded.description,
			base_scope=excluded.base_scope,
			updated_at=excluded.updated_at`,
		cp.ID, cp.Name, cp.Description, cp.BaseScope, cp.CreatedAt, cp.UpdatedAt,
	)
	return err
}

// GetContextProfileByName retrieves a context profile by its unique name (case-insensitive).
func (db *DB) GetContextProfileByName(name string) (*ContextProfile, error) {
	var cp ContextProfile
	err := db.conn.QueryRow(
		`SELECT id, name, description, base_scope, created_at, updated_at
		 FROM context_profiles WHERE name = ? COLLATE NOCASE`, name,
	).Scan(&cp.ID, &cp.Name, &cp.Description, &cp.BaseScope, &cp.CreatedAt, &cp.UpdatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &cp, nil
}

// ListContextProfiles returns all stored context profiles, ordered by name.
func (db *DB) ListContextProfiles() ([]*ContextProfile, error) {
	rows, err := db.conn.Query(
		`SELECT id, name, description, base_scope, created_at, updated_at
		 FROM context_profiles ORDER BY name COLLATE NOCASE`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var profiles []*ContextProfile
	for rows.Next() {
		var cp ContextProfile
		if err := rows.Scan(&cp.ID, &cp.Name, &cp.Description, &cp.BaseScope, &cp.CreatedAt, &cp.UpdatedAt); err != nil {
			return nil, err
		}
		profiles = append(profiles, &cp)
	}
	return profiles, nil
}

// DeleteContextProfile removes a context profile and all its links by name.
func (db *DB) DeleteContextProfile(name string) error {
	_, err := db.conn.Exec("DELETE FROM context_profiles WHERE name = ? COLLATE NOCASE", name)
	return err
}

// AddContextProfileLink adds a new link to an existing context profile.
// The parentProfileID may be nil for direct scope links.
func (db *DB) AddContextProfileLink(link *ContextProfileLink) error {
	now := time.Now().UTC()
	link.CreatedAt = now

	_, err := db.conn.Exec(
		`INSERT INTO context_profile_links
			(profile_id, parent_profile_id, scope, filter_key, filter_value, precedence_order, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		link.ProfileID, link.ParentProfileID, link.Scope, link.FilterKey, link.FilterValue, link.PrecedenceOrder, link.CreatedAt,
	)
	return err
}

// RemoveContextProfileLink removes a link by its profile name and scope.
// When the scope is empty, it removes all links for the profile.
func (db *DB) RemoveContextProfileLink(profileName, scope string) error {
	// First resolve profile ID.
	cp, err := db.GetContextProfileByName(profileName)
	if err != nil {
		return err
	}
	if cp == nil {
		return fmt.Errorf("context profile not found: %s", profileName)
	}
	if scope == "" {
		_, err = db.conn.Exec("DELETE FROM context_profile_links WHERE profile_id = ?", cp.ID)
	} else {
		_, err = db.conn.Exec("DELETE FROM context_profile_links WHERE profile_id = ? AND scope = ?", cp.ID, scope)
	}
	return err
}

// ListContextProfileLinks returns all links for a profile, ordered by precedence.
func (db *DB) ListContextProfileLinks(profileName string) ([]*ContextProfileLink, error) {
	cp, err := db.GetContextProfileByName(profileName)
	if err != nil {
		return nil, err
	}
	if cp == nil {
		return nil, nil
	}

	rows, err := db.conn.Query(
		`SELECT id, profile_id, parent_profile_id, scope, filter_key, filter_value, precedence_order, created_at
		 FROM context_profile_links WHERE profile_id = ?
		 ORDER BY precedence_order ASC, id ASC`, cp.ID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var links []*ContextProfileLink
	for rows.Next() {
		var l ContextProfileLink
		if err := rows.Scan(&l.ID, &l.ProfileID, &l.ParentProfileID, &l.Scope, &l.FilterKey, &l.FilterValue, &l.PrecedenceOrder, &l.CreatedAt); err != nil {
			return nil, err
		}
		links = append(links, &l)
	}
	return links, nil
}

// ResolveContextProfile resolves a named context profile into an ordered list
// of ResolvedScope entries. It follows parent profile links recursively,
// detecting cycles and enforcing a maximum depth.
//
// Resolution order:
//  1. Fetch the profile's links sorted by (precedence_order ASC, id ASC).
//  2. For each link:
//     a. If parent_profile_id is set, recurse into the parent first (depth+1).
//     b. Otherwise emit the link's scope + filter as a ResolvedScope.
//  3. After all links, append the profile's base_scope (if non-empty) as the
//     lowest-precedence fallback.
func (db *DB) ResolveContextProfile(name string, maxDepth int) ([]ResolvedScope, error) {
	if maxDepth <= 0 {
		maxDepth = DefaultMaxDepth
	}
	visited := make(map[string]bool)
	return db.resolveContextProfileInternal(name, maxDepth, 0, visited)
}

func (db *DB) resolveContextProfileInternal(name string, maxDepth, depth int, visited map[string]bool) ([]ResolvedScope, error) {
	cp, err := db.GetContextProfileByName(name)
	if err != nil {
		return nil, err
	}
	if cp == nil {
		return nil, fmt.Errorf("context profile not found: %s", name)
	}

	if depth >= maxDepth {
		return nil, fmt.Errorf("context profile %q: inheritance depth %d exceeds maximum %d", name, depth+1, maxDepth)
	}

	if visited[cp.ID] {
		return nil, fmt.Errorf("context profile %q: cycle detected involving profile %q", name, name)
	}
	visited[cp.ID] = true
	defer delete(visited, cp.ID)

	links, err := db.ListContextProfileLinks(name)
	if err != nil {
		return nil, err
	}

	var resolved []ResolvedScope
	for _, l := range links {
		if l.ParentProfileID != nil && *l.ParentProfileID != "" {
			// Resolve the parent profile by looking up its name.
			parentCP, perr := db.getContextProfileByID(*l.ParentProfileID)
			if perr != nil {
				return nil, perr
			}
			if parentCP == nil {
				// Parent was deleted or missing; skip this link.
				continue
			}
			parentScopes, perr := db.resolveContextProfileInternal(parentCP.Name, maxDepth, depth+1, visited)
			if perr != nil {
				return nil, perr
			}
			resolved = append(resolved, parentScopes...)
		}
		if l.Scope != "" {
			resolved = append(resolved, ResolvedScope{
				Scope:       l.Scope,
				Profile:     name,
				FilterKey:   l.FilterKey,
				FilterValue: l.FilterValue,
			})
		}
	}

	// Append base scope as the lowest-precedence fallback.
	if cp.BaseScope != "" {
		resolved = append(resolved, ResolvedScope{
			Scope:   cp.BaseScope,
			Profile: name,
		})
	}

	return resolved, nil
}

// getContextProfileByID retrieves a context profile by its primary key.
func (db *DB) getContextProfileByID(id string) (*ContextProfile, error) {
	var cp ContextProfile
	err := db.conn.QueryRow(
		`SELECT id, name, description, base_scope, created_at, updated_at
		 FROM context_profiles WHERE id = ?`, id,
	).Scan(&cp.ID, &cp.Name, &cp.Description, &cp.BaseScope, &cp.CreatedAt, &cp.UpdatedAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &cp, nil
}
