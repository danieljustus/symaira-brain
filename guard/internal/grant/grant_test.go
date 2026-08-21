package grant

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func testGrant(id, subject string, scope Scope) *Grant {
	return &Grant{
		ID:        id,
		Scope:     scope,
		Origin:    Origin{Epoch: 1722924000, Via: "approval"},
		GrantedAt: time.Date(2026, 8, 6, 10, 0, 0, 0, time.UTC),
		Subject:   subject,
	}
}

func TestNewID(t *testing.T) {
	seen := make(map[string]bool)
	for i := 0; i < 1000; i++ {
		id := NewID()
		if seen[id] {
			t.Fatalf("NewID() returned duplicate %q", id)
		}
		seen[id] = true
		if !strings.HasPrefix(id, "gnt_") {
			t.Errorf("NewID() = %q, want gnt_ prefix", id)
		}
		parts := strings.Split(id, "_")
		if len(parts) != 3 || parts[0] != "gnt" || len(parts[2]) != 8 {
			t.Errorf("NewID() = %q, want shape gnt_<nanos>_<8-hex>", id)
		}
	}
}

func TestNewID_RandFailureFallback(t *testing.T) {
	old := randRead
	t.Cleanup(func() { randRead = old })
	randRead = func(b []byte) (int, error) { return 0, errors.New("entropy unavailable") }

	id := NewID()
	if !strings.HasPrefix(id, "gnt_") {
		t.Errorf("NewID() = %q, want gnt_ prefix", id)
	}
	// Fallback shape: timestamp only, no hex suffix.
	if strings.Count(id, "_") != 1 {
		t.Errorf("NewID() = %q, want timestamp-only fallback shape", id)
	}
}

func TestScope_Valid(t *testing.T) {
	tests := []struct {
		name  string
		scope Scope
		want  bool
	}{
		{"run", ScopeRun, true},
		{"session", ScopeSession, true},
		{"device", ScopeDevice, true},
		{"vault", ScopeVault, true},
		{"empty", Scope(""), false},
		{"unknown", Scope("cluster"), false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.scope.Valid(); got != tt.want {
				t.Errorf("Valid() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestScope_Persistent(t *testing.T) {
	tests := []struct {
		name  string
		scope Scope
		want  bool
	}{
		{"run is in-memory", ScopeRun, false},
		{"session is in-memory", ScopeSession, false},
		{"device is persisted", ScopeDevice, true},
		{"vault is persisted", ScopeVault, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.scope.Persistent(); got != tt.want {
				t.Errorf("Persistent() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestStore_AddValidation(t *testing.T) {
	st, err := Open(t.TempDir())
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	tests := []struct {
		name    string
		grant   *Grant
		wantErr bool
	}{
		{"valid device grant", testGrant("g1", "agent-1", ScopeDevice), false},
		{"valid session grant", testGrant("g2", "agent-1", ScopeSession), false},
		{"nil grant", nil, true},
		{"empty ID", testGrant("", "agent-1", ScopeSession), true},
		{"unknown scope", testGrant("g3", "agent-1", Scope("cluster")), true},
		{"empty subject", testGrant("g4", "", ScopeSession), true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := st.Add(tt.grant)
			if (err != nil) != tt.wantErr {
				t.Errorf("Add() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestStore_ActiveAndAll(t *testing.T) {
	st, err := Open(t.TempDir())
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	g1 := testGrant("g1", "agent-1", ScopeSession)
	g2 := testGrant("g2", "agent-2", ScopeDevice)
	g3 := testGrant("g3", "agent-1", ScopeVault)
	g2.GrantedAt = g1.GrantedAt.Add(time.Minute)
	g3.GrantedAt = g2.GrantedAt.Add(time.Minute)
	for _, g := range []*Grant{g1, g2, g3} {
		if err := st.Add(g); err != nil {
			t.Fatalf("Add(%s) error = %v", g.ID, err)
		}
	}
	if err := g2.Revoke(); err != nil {
		t.Fatalf("Revoke() error = %v", err)
	}

	active := st.Active()
	if len(active) != 2 {
		t.Fatalf("Active() = %d grants, want 2", len(active))
	}
	// Newest first: g3 granted last.
	if active[0].ID != "g3" || active[1].ID != "g1" {
		t.Errorf("Active() order = [%s, %s], want [g3, g1]", active[0].ID, active[1].ID)
	}

	all := st.All()
	if len(all) != 3 {
		t.Fatalf("All() = %d grants, want 3 (tombstone kept)", len(all))
	}
}

func TestStore_RevokeByID(t *testing.T) {
	st, err := Open(t.TempDir())
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	if err := st.Add(testGrant("g1", "agent-1", ScopeSession)); err != nil {
		t.Fatalf("Add() error = %v", err)
	}

	if err := st.Revoke("missing"); err == nil {
		t.Error("Revoke(missing) = nil, want ErrNotFound")
	} else if !strings.Contains(err.Error(), "not found") {
		t.Errorf("Revoke(missing) error = %v, want ErrNotFound", err)
	}

	if err := st.Revoke("g1"); err != nil {
		t.Fatalf("Revoke(g1) error = %v", err)
	}
	if len(st.Active()) != 0 {
		t.Error("Active() not empty after revoke")
	}
	if g, ok := st.Get("g1"); !ok || !g.Revoked {
		t.Error("Get(g1) should return the revoked tombstone")
	}
	// Revoking twice is a no-op, not an error.
	if err := st.Revoke("g1"); err != nil {
		t.Errorf("second Revoke(g1) error = %v, want nil", err)
	}
}

func TestStore_RevokeAll(t *testing.T) {
	st, err := Open(t.TempDir())
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	for _, id := range []string{"g1", "g2", "g3"} {
		if err := st.Add(testGrant(id, "agent-1", ScopeSession)); err != nil {
			t.Fatalf("Add(%s) error = %v", id, err)
		}
	}
	n, err := st.RevokeAll()
	if err != nil {
		t.Fatalf("RevokeAll() error = %v", err)
	}
	if n != 3 {
		t.Errorf("RevokeAll() = %d, want 3", n)
	}
	if len(st.Active()) != 0 {
		t.Error("Active() not empty after RevokeAll")
	}
}

func TestStore_ActiveForSubject(t *testing.T) {
	st, err := Open(t.TempDir())
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	for _, g := range []*Grant{
		testGrant("g1", "agent-1", ScopeSession),
		testGrant("g2", "agent-2", ScopeDevice),
		testGrant("g3", "agent-1", ScopeVault),
	} {
		if err := st.Add(g); err != nil {
			t.Fatalf("Add(%s) error = %v", g.ID, err)
		}
	}
	if err := st.Revoke("g3"); err != nil {
		t.Fatalf("Revoke(g3) error = %v", err)
	}

	got := st.ActiveForSubject("agent-1")
	if len(got) != 1 || got[0].ID != "g1" {
		t.Errorf("ActiveForSubject(agent-1) = %v, want [g1]", ids(got))
	}
	if got := st.ActiveForSubject("agent-2"); len(got) != 1 || got[0].ID != "g2" {
		t.Errorf("ActiveForSubject(agent-2) = %v, want [g2]", ids(got))
	}
	if got := st.ActiveForSubject("nobody"); len(got) != 0 {
		t.Errorf("ActiveForSubject(nobody) = %v, want []", ids(got))
	}
}

func TestStore_Persistence(t *testing.T) {
	dir := t.TempDir()
	st, err := Open(dir)
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	device := testGrant("g-device", "agent-1", ScopeDevice)
	session := testGrant("g-session", "agent-1", ScopeSession)
	if err := st.Add(device); err != nil {
		t.Fatalf("Add(device) error = %v", err)
	}
	if err := st.Add(session); err != nil {
		t.Fatalf("Add(session) error = %v", err)
	}

	// Reopen: the device grant survives, the session grant does not.
	st2, err := Open(dir)
	if err != nil {
		t.Fatalf("reopen Open() error = %v", err)
	}
	if _, ok := st2.Get("g-device"); !ok {
		t.Error("device grant lost after reopen")
	}
	if _, ok := st2.Get("g-session"); ok {
		t.Error("session grant persisted, want in-memory only")
	}
}

func TestStore_PersistedRevocationIsTombstone(t *testing.T) {
	dir := t.TempDir()
	st, err := Open(dir)
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	if err := st.Add(testGrant("g-device", "agent-1", ScopeDevice)); err != nil {
		t.Fatalf("Add() error = %v", err)
	}
	if err := st.Revoke("g-device"); err != nil {
		t.Fatalf("Revoke() error = %v", err)
	}

	st2, err := Open(dir)
	if err != nil {
		t.Fatalf("reopen Open() error = %v", err)
	}
	g, ok := st2.Get("g-device")
	if !ok {
		t.Fatal("revoked device grant missing after reopen")
	}
	if !g.Revoked {
		t.Error("revoked device grant not marked revoked after reopen")
	}
	if len(st2.Active()) != 0 {
		t.Error("Active() contains revoked grant after reopen")
	}
}

func TestStore_LoadFailsClosed(t *testing.T) {
	tests := []struct {
		name    string
		content string
	}{
		{"malformed json", "{not json"},
		{"unknown scope", `[{"id":"g1","scope":"cluster","subject":"a"}]`},
		{"duplicate id", `[{"id":"g1","scope":"device","subject":"a"},{"id":"g1","scope":"vault","subject":"b"}]`},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			if err := os.WriteFile(filepath.Join(dir, "grants.json"), []byte(tt.content), 0o600); err != nil {
				t.Fatalf("WriteFile() error = %v", err)
			}
			if _, err := Open(dir); err == nil {
				t.Error("Open() = nil error, want fail-closed error")
			}
		})
	}
}

func TestDefaultDir(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")
	t.Setenv("SYMGUARD_DATA", "")
	if got := DefaultDir(); got != filepath.Join(home, ".local", "share", "symguard") {
		t.Errorf("DefaultDir() = %q, want XDG fallback path", got)
	}
	t.Setenv("XDG_DATA_HOME", "/xdg")
	if got := DefaultDir(); got != filepath.Join("/xdg", "symguard") {
		t.Errorf("DefaultDir() = %q, want XDG_DATA_HOME path", got)
	}
	t.Setenv("SYMGUARD_DATA", "/override")
	if got := DefaultDir(); got != "/override" {
		t.Errorf("DefaultDir() = %q, want SYMGUARD_DATA override", got)
	}
}

func TestGrant_RevokeDetached(t *testing.T) {
	// A grant that was never attached to a store can still be revoked
	// locally; the revocation is simply not persisted anywhere.
	g := testGrant("g1", "agent-1", ScopeSession)
	if err := g.Revoke(); err != nil {
		t.Fatalf("Revoke() error = %v", err)
	}
	if !g.Revoked {
		t.Error("grant not marked revoked")
	}
	if err := g.Revoke(); err != nil {
		t.Errorf("second Revoke() error = %v, want nil", err)
	}
}

func ids(gs []*Grant) []string {
	out := make([]string, 0, len(gs))
	for _, g := range gs {
		out = append(out, g.ID)
	}
	return out
}
