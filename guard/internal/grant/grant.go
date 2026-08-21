// Package grant defines the enumerable, scoped, revocable grant model for
// symguard's approval layer.
//
// A grant is a standing permission: unlike a consumed approval decision or a
// rule match, it survives the request that created it and can be listed,
// scoped, and revoked. The store answers "what have I allowed, how far does
// it reach, and how do I take it back?" from a single source of truth, so the
// kill switch (`grants revoke`) and the settings surface (`grants list`)
// enumerate the same set instead of drifting apart.
//
// The pattern follows vault-operator's flat grant list (pattern only, no
// code): every entry carries a Scope (run | session | device | vault, ordered
// widest to narrowest) and an Origin with an epoch timestamp, and every entry
// has a revoke function.
//
// # Persistence
//
// Grants scoped to run or session are ephemeral and live in memory only.
// Grants scoped to device or vault are persisted as JSON under the XDG data
// directory (~/.local/share/symguard/grants.json, with $XDG_DATA_HOME and
// $SYMGUARD_DATA overrides). Revoked grants are kept as tombstones so the
// history of what was taken back remains visible.
//
// # Taxonomy coordination
//
// Scope and Origin are the shared taxonomy for symguard and symaira-brain —
// both layers need the same taxonomy. They are defined here as candidates
// for symaira-corekit later; this package intentionally does not import
// symaira-corekit.
package grant

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-guard/internal/config"
)

// ErrNotFound is returned when a grant ID is not present in the store.
var ErrNotFound = errors.New("grant: not found")

// Scope is the reach of a grant, ordered widest to narrowest:
// run (a single agent run), session (one interactive session),
// device (this machine), vault (a specific vault).
type Scope string

const (
	ScopeRun     Scope = "run"
	ScopeSession Scope = "session"
	ScopeDevice  Scope = "device"
	ScopeVault   Scope = "vault"
)

// Valid reports whether s is a known scope.
func (s Scope) Valid() bool {
	switch s {
	case ScopeRun, ScopeSession, ScopeDevice, ScopeVault:
		return true
	}
	return false
}

// Persistent reports whether grants of this scope outlive the process.
// run and session are in-memory only; device and vault are persisted.
func (s Scope) Persistent() bool {
	switch s {
	case ScopeDevice, ScopeVault:
		return true
	}
	return false
}

// Origin records where a grant came from and when. Epoch is the unix
// timestamp (seconds) at which the grant originated.
type Origin struct {
	Epoch int64  `json:"epoch"`
	Via   string `json:"via,omitempty"` // originating layer, e.g. "approval"
}

// Grant is one standing permission.
type Grant struct {
	ID        string    `json:"id"`
	Scope     Scope     `json:"scope"`
	Origin    Origin    `json:"origin"`
	GrantedAt time.Time `json:"granted_at"`
	Subject   string    `json:"subject"`
	Revoked   bool      `json:"revoked,omitempty"`

	// revoke is attached by the store so Revoke() reaches the registry;
	// it is never serialized.
	revoke func(g *Grant) error `json:"-"`
}

// Revoke marks the grant revoked and, when the grant is attached to a store,
// removes it from the active set and persists the revocation. Revoking an
// already revoked grant is a no-op.
func (g *Grant) Revoke() error {
	if g.Revoked {
		return nil
	}
	g.Revoked = true
	if g.revoke != nil {
		return g.revoke(g)
	}
	return nil
}

// randRead is the entropy source for grant IDs. It is a variable so the
// crypto/rand failure fallback is testable.
var randRead = rand.Read

// NewID returns a fresh, locally unique grant ID.
func NewID() string {
	var b [4]byte
	if _, err := randRead(b[:]); err != nil {
		// crypto/rand failure is unrecoverable; the timestamp alone still
		// provides local uniqueness.
		return fmt.Sprintf("gnt_%d", time.Now().UnixNano())
	}
	return fmt.Sprintf("gnt_%d_%s", time.Now().UnixNano(), hex.EncodeToString(b[:]))
}

// DefaultDir returns the XDG data directory for symguard:
//
//	$SYMGUARD_DATA, else $XDG_DATA_HOME/symguard, else ~/.local/share/symguard.
func DefaultDir() string {
	if env := os.Getenv("SYMGUARD_DATA"); env != "" {
		return env
	}
	return config.DataDir()
}
