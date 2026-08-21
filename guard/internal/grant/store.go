package grant

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"sync"
)

// Store is the grant registry. It is safe for concurrent use.
//
// Persistent-scope grants (device, vault) are written through to
// dir/grants.json on every mutation; run and session grants live in memory
// only and disappear with the process.
type Store struct {
	dir    string
	mu     sync.Mutex
	grants map[string]*Grant
}

// Open loads the grant store rooted at dir, creating dir when missing.
// Persisted grants are read from dir/grants.json; a corrupt or invalid file
// fails closed with an error.
func Open(dir string) (*Store, error) {
	s := &Store{dir: dir, grants: make(map[string]*Grant)}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, fmt.Errorf("grant: create store dir %s: %w", dir, err)
	}
	if err := s.load(); err != nil {
		return nil, err
	}
	return s, nil
}

// Add registers a grant in the store. Grants with persistent scopes
// (device, vault) are written to disk immediately; run and session grants
// stay in memory only. A grant with an ID already in the store replaces the
// previous entry.
func (s *Store) Add(g *Grant) error {
	if g == nil {
		return errors.New("grant: add nil grant")
	}
	if g.ID == "" {
		return errors.New("grant: add grant with empty ID")
	}
	if !g.Scope.Valid() {
		return fmt.Errorf("grant: add grant with unknown scope %q", g.Scope)
	}
	if g.Subject == "" {
		return errors.New("grant: add grant with empty subject")
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	g.revoke = s.revokeHook
	s.grants[g.ID] = g
	if g.Scope.Persistent() && !g.Revoked {
		return s.persistLocked()
	}
	return nil
}

// Get returns the grant with the given ID, whether active or revoked.
func (s *Store) Get(id string) (*Grant, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	g, ok := s.grants[id]
	return g, ok
}

// Active returns the non-revoked grants, newest first.
func (s *Store) Active() []*Grant {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.activeLocked()
}

// All returns every known grant — active and revoked tombstones — newest
// first. It is the full history surface for settings and audit views.
func (s *Store) All() []*Grant {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]*Grant, 0, len(s.grants))
	for _, g := range s.grants {
		out = append(out, g)
	}
	sortGrants(out)
	return out
}

// ActiveForSubject returns the active grants held by subject, newest first.
// It is the read path for the policy engine's grant consult.
func (s *Store) ActiveForSubject(subject string) []*Grant {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []*Grant
	for _, g := range s.grants {
		if !g.Revoked && g.Subject == subject {
			out = append(out, g)
		}
	}
	sortGrants(out)
	return out
}

// Revoke revokes the grant with the given ID. It returns ErrNotFound when no
// grant with that ID is known; revoking an already revoked grant is a no-op.
func (s *Store) Revoke(id string) error {
	s.mu.Lock()
	g, ok := s.grants[id]
	s.mu.Unlock()
	if !ok {
		return fmt.Errorf("%w: %s", ErrNotFound, id)
	}
	return g.Revoke()
}

// RevokeAll revokes every active grant and returns how many were revoked.
func (s *Store) RevokeAll() (int, error) {
	active := s.Active()
	for _, g := range active {
		if err := g.Revoke(); err != nil {
			return 0, err
		}
	}
	return len(active), nil
}

// activeLocked returns non-revoked grants, newest first. Caller holds mu.
func (s *Store) activeLocked() []*Grant {
	var out []*Grant
	for _, g := range s.grants {
		if !g.Revoked {
			out = append(out, g)
		}
	}
	sortGrants(out)
	return out
}

// revokeHook is attached to every grant the store registers or loads. For
// persistent scopes it persists the revocation (keeping the tombstone); for
// in-memory scopes the revocation is already effective in the registry.
func (s *Store) revokeHook(g *Grant) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if g.Scope.Persistent() {
		return s.persistLocked()
	}
	return nil
}

// load reads dir/grants.json into the registry. A missing file is an empty
// store; a malformed file, duplicate ID, or unknown scope fails closed.
func (s *Store) load() error {
	path := s.file()
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("grant: read %s: %w", path, err)
	}
	var entries []Grant
	if err := json.Unmarshal(data, &entries); err != nil {
		return fmt.Errorf("grant: parse %s: %w", path, err)
	}
	for i := range entries {
		g := &entries[i]
		if !g.Scope.Valid() {
			return fmt.Errorf("grant: parse %s: unknown scope %q", path, g.Scope)
		}
		if _, dup := s.grants[g.ID]; dup {
			return fmt.Errorf("grant: parse %s: duplicate grant ID %q", path, g.ID)
		}
		g.revoke = s.revokeHook
		s.grants[g.ID] = g
	}
	return nil
}

// persistLocked writes all persistent-scope grants (including revoked
// tombstones) to dir/grants.json atomically. Caller holds mu.
func (s *Store) persistLocked() error {
	var entries []Grant
	ids := make([]string, 0, len(s.grants))
	for id, g := range s.grants {
		if g.Scope.Persistent() {
			ids = append(ids, id)
		}
	}
	sort.Strings(ids)
	for _, id := range ids {
		entries = append(entries, *s.grants[id])
	}
	data, err := json.MarshalIndent(entries, "", "  ")
	if err != nil {
		return fmt.Errorf("grant: marshal store: %w", err)
	}
	tmp, err := os.CreateTemp(s.dir, ".grants-*.tmp")
	if err != nil {
		return fmt.Errorf("grant: create temp file: %w", err)
	}
	tmpName := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		os.Remove(tmpName)
		return fmt.Errorf("grant: write temp file: %w", err)
	}
	if err := tmp.Close(); err != nil {
		os.Remove(tmpName)
		return fmt.Errorf("grant: close temp file: %w", err)
	}
	if err := os.Rename(tmpName, s.file()); err != nil {
		os.Remove(tmpName)
		return fmt.Errorf("grant: rename temp file: %w", err)
	}
	return nil
}

func (s *Store) file() string {
	return filepath.Join(s.dir, "grants.json")
}

// sortGrants orders grants newest first by GrantedAt, falling back to ID for
// stable ordering of equal timestamps.
func sortGrants(gs []*Grant) {
	sort.Slice(gs, func(i, j int) bool {
		if !gs[i].GrantedAt.Equal(gs[j].GrantedAt) {
			return gs[i].GrantedAt.After(gs[j].GrantedAt)
		}
		return gs[i].ID < gs[j].ID
	})
}
