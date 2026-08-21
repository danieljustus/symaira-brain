package grants

import (
	"bytes"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-guard/internal/grant"
)

// openStore creates a store in a temp dir and points SYMGUARD_DATA at it so
// Run() resolves the same path as a real CLI invocation. It returns the dir
// as well, so tests can re-open the store and observe persisted state the
// way a second process would.
func openStore(t *testing.T) (string, *grant.Store) {
	t.Helper()
	dir := t.TempDir()
	t.Setenv("SYMGUARD_DATA", dir)
	st, err := grant.Open(dir)
	if err != nil {
		t.Fatalf("grant.Open() error = %v", err)
	}
	return dir, st
}

func reopen(t *testing.T, dir string) *grant.Store {
	t.Helper()
	st, err := grant.Open(dir)
	if err != nil {
		t.Fatalf("reopen grant.Open() error = %v", err)
	}
	return st
}

func addGrant(t *testing.T, st *grant.Store, id, subject string, scope grant.Scope) {
	t.Helper()
	if err := st.Add(&grant.Grant{
		ID:        id,
		Scope:     scope,
		Origin:    grant.Origin{Epoch: 1722924000, Via: "approval"},
		GrantedAt: time.Date(2026, 8, 6, 10, 0, 0, 0, time.UTC),
		Subject:   subject,
	}); err != nil {
		t.Fatalf("store.Add(%s) error = %v", id, err)
	}
}

func TestRun_ListEmpty(t *testing.T) {
	openStore(t)
	var buf bytes.Buffer
	Run([]string{"list"}, &buf)
	if !strings.Contains(buf.String(), "No active grants.") {
		t.Errorf("expected empty list message, got: %s", buf.String())
	}
}

func TestRun_ListShowsGrants(t *testing.T) {
	_, st := openStore(t)
	// Persistent scopes only: the CLI opens its own store instance, so
	// in-memory (session/run) grants would not be visible to it.
	addGrant(t, st, "gnt_1", "agent-1", grant.ScopeDevice)
	addGrant(t, st, "gnt_2", "agent-2", grant.ScopeVault)

	var buf bytes.Buffer
	Run([]string{"list"}, &buf)
	out := buf.String()
	for _, want := range []string{"ID", "SCOPE", "gnt_1", "device", "agent-1", "gnt_2", "vault", "approval@1722924000"} {
		if !strings.Contains(out, want) {
			t.Errorf("list output missing %q:\n%s", want, out)
		}
	}
}

func TestRun_RevokeByID(t *testing.T) {
	dir, st := openStore(t)
	addGrant(t, st, "gnt_1", "agent-1", grant.ScopeDevice)

	var buf bytes.Buffer
	Run([]string{"revoke", "gnt_1"}, &buf)
	out := buf.String()
	if !strings.Contains(out, "Revoked grant gnt_1.") {
		t.Errorf("expected revoke confirmation, got: %s", out)
	}
	// A fresh store instance (as a second invocation would see) must no
	// longer list the grant as active.
	fresh := reopen(t, dir)
	if got := fresh.Active(); len(got) != 0 {
		t.Errorf("active grants after revoke = %v, want none", got)
	}
	if g, ok := fresh.Get("gnt_1"); !ok || !g.Revoked {
		t.Error("revoked grant missing or not marked revoked after reopen")
	}
}

func TestRun_RevokeUnknownID(t *testing.T) {
	openStore(t)
	var buf bytes.Buffer
	Run([]string{"revoke", "missing"}, &buf)
	out := buf.String()
	if !strings.Contains(out, "not found") {
		t.Errorf("expected not-found error, got: %s", out)
	}
}

func TestRun_RevokeAll(t *testing.T) {
	dir, st := openStore(t)
	addGrant(t, st, "gnt_1", "agent-1", grant.ScopeDevice)
	addGrant(t, st, "gnt_2", "agent-2", grant.ScopeDevice)
	addGrant(t, st, "gnt_3", "agent-3", grant.ScopeVault)

	var buf bytes.Buffer
	Run([]string{"revoke", "--all"}, &buf)
	out := buf.String()
	if !strings.Contains(out, "Revoked 3 grant(s).") {
		t.Errorf("expected revoke-all confirmation, got: %s", out)
	}
	fresh := reopen(t, dir)
	if got := fresh.Active(); len(got) != 0 {
		t.Errorf("active grants after revoke-all = %v, want none", got)
	}
	if got := fresh.All(); len(got) != 3 {
		t.Errorf("tombstones after revoke-all = %d, want 3", len(got))
	}
}

func TestRun_RevokeUsageErrors(t *testing.T) {
	tests := []struct {
		name    string
		args    []string
		wantOut string
	}{
		{"missing id", []string{"revoke"}, "missing grant ID or --all"},
		{"all with id", []string{"revoke", "--all", "gnt_1"}, "cannot be combined"},
		{"unexpected flag", []string{"revoke", "--bogus"}, "unexpected flag"},
		{"too many args", []string{"revoke", "gnt_1", "gnt_2"}, "unexpected argument"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			openStore(t)
			var buf bytes.Buffer
			Run(tt.args, &buf)
			out := buf.String()
			if !strings.Contains(out, tt.wantOut) {
				t.Errorf("output missing %q, got: %s", tt.wantOut, out)
			}
			if !strings.Contains(out, "Usage:") {
				t.Errorf("expected usage on error, got: %s", out)
			}
		})
	}
}

func TestRun_Usage(t *testing.T) {
	tests := []struct {
		name string
		args []string
	}{
		{"no args", nil},
		{"help", []string{"--help"}},
		{"short help", []string{"-h"}},
		{"help subcommand", []string{"help"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			Run(tt.args, &buf)
			out := buf.String()
			for _, want := range []string{"Usage:", "list", "revoke"} {
				if !strings.Contains(out, want) {
					t.Errorf("usage output missing %q, got: %s", want, out)
				}
			}
		})
	}
}

func TestRun_UnknownSubcommand(t *testing.T) {
	var buf bytes.Buffer
	Run([]string{"bogus"}, &buf)
	out := buf.String()
	if !strings.Contains(out, "unknown grants subcommand: bogus") {
		t.Errorf("expected unknown subcommand error, got: %s", out)
	}
}
