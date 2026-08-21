package policy

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/grant"
	"github.com/danieljustus/symaira-guard/internal/model"
)

// fakeLookup implements GrantLookup for tests.
type fakeLookup struct {
	subjects map[string][]*grant.Grant
}

func (f fakeLookup) ActiveForSubject(subject string) []*grant.Grant {
	return f.subjects[subject]
}

func grantsFor(subject string) fakeLookup {
	return fakeLookup{subjects: map[string][]*grant.Grant{
		subject: {{ID: "g1", Subject: subject, Scope: grant.ScopeSession}},
	}}
}

func mustCatalog(t *testing.T, rules ...Rule) *Catalog {
	t.Helper()
	c, err := NewCatalog(rules, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog() error = %v", err)
	}
	return c
}

func TestEvaluateWithGrants(t *testing.T) {
	call := model.ToolCall{Server: "fs", Tool: "read_file", Capability: "read_secret"}
	askRule := Rule{ID: "ask-1", Version: "1.0", Precedence: 10, Decision: model.DecisionAsk,
		Match: MatchCriteria{Capability: "read_secret"}}
	denyRule := Rule{ID: "deny-1", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
		Match: MatchCriteria{Capability: "read_secret"}}
	allowRule := Rule{ID: "allow-1", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow,
		Match: MatchCriteria{Capability: "read_secret"}}
	otherRule := Rule{ID: "other-1", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow,
		Match: MatchCriteria{Capability: "network"}}

	tests := []struct {
		name     string
		catalog  *Catalog
		subject  string
		lookup   GrantLookup
		defaults model.Decision
		want     model.Decision
	}{
		{
			name:    "ask upgraded to allow by standing grant",
			catalog: mustCatalog(t, askRule),
			subject: "agent-1", lookup: grantsFor("agent-1"),
			defaults: model.DecisionAsk, want: model.DecisionAllow,
		},
		{
			name:    "ask without grant stays ask",
			catalog: mustCatalog(t, askRule),
			subject: "agent-1", lookup: fakeLookup{},
			defaults: model.DecisionAsk, want: model.DecisionAsk,
		},
		{
			name:    "ask with empty subject stays ask",
			catalog: mustCatalog(t, askRule),
			subject: "", lookup: grantsFor("agent-1"),
			defaults: model.DecisionAsk, want: model.DecisionAsk,
		},
		{
			name:    "ask with nil lookup stays ask",
			catalog: mustCatalog(t, askRule),
			subject: "agent-1", lookup: nil,
			defaults: model.DecisionAsk, want: model.DecisionAsk,
		},
		{
			name:    "deny is never overridden by grant",
			catalog: mustCatalog(t, denyRule),
			subject: "agent-1", lookup: grantsFor("agent-1"),
			defaults: model.DecisionAsk, want: model.DecisionDeny,
		},
		{
			name:    "explicit allow is unchanged",
			catalog: mustCatalog(t, allowRule),
			subject: "agent-1", lookup: grantsFor("agent-1"),
			defaults: model.DecisionAsk, want: model.DecisionAllow,
		},
		{
			name:    "default ask upgraded when subject holds grant",
			catalog: mustCatalog(t, otherRule),
			subject: "agent-1", lookup: grantsFor("agent-1"),
			defaults: model.DecisionAsk, want: model.DecisionAllow,
		},
		{
			name:    "default ask without grant stays ask",
			catalog: mustCatalog(t, otherRule),
			subject: "agent-1", lookup: fakeLookup{},
			defaults: model.DecisionAsk, want: model.DecisionAsk,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.catalog.EvaluateWithGrants(tt.subject, call, tt.defaults, tt.lookup)
			if got.Decision != tt.want {
				t.Errorf("EvaluateWithGrants() decision = %q, want %q (reason %q)",
					got.Decision, tt.want, got.Reason)
			}
		})
	}
}

func TestEvaluateWithGrants_RealStore(t *testing.T) {
	// The store satisfies the consult seam end to end.
	st, err := grant.Open(t.TempDir())
	if err != nil {
		t.Fatalf("grant.Open() error = %v", err)
	}
	if err := st.Add(&grant.Grant{
		ID: "g1", Scope: grant.ScopeSession, Subject: "agent-1",
		Origin: grant.Origin{Epoch: 1, Via: "approval"},
	}); err != nil {
		t.Fatalf("store.Add() error = %v", err)
	}

	c := mustCatalog(t, Rule{ID: "ask-1", Version: "1.0", Precedence: 10,
		Decision: model.DecisionAsk, Match: MatchCriteria{Capability: "read_secret"}})
	call := model.ToolCall{Capability: "read_secret"}

	got := c.EvaluateWithGrants("agent-1", call, model.DecisionAsk, st)
	if got.Decision != model.DecisionAllow {
		t.Errorf("EvaluateWithGrants() = %q, want %q", got.Decision, model.DecisionAllow)
	}

	if err := st.Revoke("g1"); err != nil {
		t.Fatalf("store.Revoke() error = %v", err)
	}
	got = c.EvaluateWithGrants("agent-1", call, model.DecisionAsk, st)
	if got.Decision != model.DecisionAsk {
		t.Errorf("after revoke EvaluateWithGrants() = %q, want %q", got.Decision, model.DecisionAsk)
	}
}
