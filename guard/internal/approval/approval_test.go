package approval

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-guard/internal/grant"
)

func TestDecision_Grant(t *testing.T) {
	decided := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	tests := []struct {
		name      string
		dec       Decision
		subject   string
		wantNil   bool
		wantErr   bool
		wantScope grant.Scope
	}{
		{
			name:      "approved with ttl creates session grant",
			dec:       Decision{ID: "req-1", Approved: true, TTL: 5 * time.Minute, DecidedAt: decided},
			subject:   "agent-1",
			wantScope: grant.ScopeSession,
		},
		{
			name:    "zero ttl keeps opaque timer path",
			dec:     Decision{ID: "req-2", Approved: true, TTL: 0, DecidedAt: decided},
			subject: "agent-1",
			wantNil: true,
		},
		{
			name:    "denied decision creates no grant",
			dec:     Decision{ID: "req-3", Approved: false, TTL: 5 * time.Minute, DecidedAt: decided},
			subject: "agent-1",
			wantNil: true,
		},
		{
			name:    "empty subject is rejected",
			dec:     Decision{ID: "req-4", Approved: true, TTL: 5 * time.Minute, DecidedAt: decided},
			subject: "",
			wantNil: true,
			wantErr: true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			g, err := tt.dec.Grant(tt.subject)
			if (err != nil) != tt.wantErr {
				t.Fatalf("Grant() error = %v, wantErr %v", err, tt.wantErr)
			}
			if tt.wantNil {
				if g != nil {
					t.Errorf("Grant() = %v, want nil", g)
				}
				return
			}
			if g == nil {
				t.Fatal("Grant() = nil, want grant")
			}
			if g.ID == "" {
				t.Error("grant ID is empty")
			}
			if g.Scope != tt.wantScope {
				t.Errorf("grant scope = %q, want %q", g.Scope, tt.wantScope)
			}
			if g.Subject != tt.subject {
				t.Errorf("grant subject = %q, want %q", g.Subject, tt.subject)
			}
			if g.Origin.Via != "approval" {
				t.Errorf("grant origin via = %q, want %q", g.Origin.Via, "approval")
			}
			if g.Origin.Epoch != decided.Unix() {
				t.Errorf("grant origin epoch = %d, want %d", g.Origin.Epoch, decided.Unix())
			}
			if !g.GrantedAt.Equal(decided) {
				t.Errorf("grant granted_at = %v, want %v", g.GrantedAt, decided)
			}
			if g.Revoked {
				t.Error("fresh grant is already revoked")
			}
		})
	}
}

func TestDecision_GrantEndToEnd(t *testing.T) {
	// A TTL decision lands in the store as a session grant and can be
	// revoked from the store side and from the grant side alike.
	st, err := grant.Open(t.TempDir())
	if err != nil {
		t.Fatalf("grant.Open() error = %v", err)
	}
	dec := Decision{ID: "req-1", Approved: true, TTL: 10 * time.Minute, DecidedAt: time.Now()}
	g, err := dec.Grant("agent-1")
	if err != nil {
		t.Fatalf("Grant() error = %v", err)
	}
	if err := st.Add(g); err != nil {
		t.Fatalf("store.Add() error = %v", err)
	}
	if got := st.ActiveForSubject("agent-1"); len(got) != 1 {
		t.Fatalf("ActiveForSubject() = %d grants, want 1", len(got))
	}

	if err := st.Revoke(g.ID); err != nil {
		t.Fatalf("store.Revoke() error = %v", err)
	}
	if got := st.Active(); len(got) != 0 {
		t.Errorf("Active() = %d grants after revoke, want 0", len(got))
	}

	g2, err := dec.Grant("agent-2")
	if err != nil {
		t.Fatalf("second Grant() error = %v", err)
	}
	if err := st.Add(g2); err != nil {
		t.Fatalf("store.Add(g2) error = %v", err)
	}
	if err := g2.Revoke(); err != nil {
		t.Fatalf("grant.Revoke() error = %v", err)
	}
	if got := st.Active(); len(got) != 0 {
		t.Errorf("Active() = %d grants after grant-side revoke, want 0", len(got))
	}
}
