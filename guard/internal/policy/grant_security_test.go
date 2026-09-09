package policy

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/grant"
	"github.com/danieljustus/symaira-brain/guard/internal/model"
)

func TestEvaluateWithGrants_RequiresExactBinding(t *testing.T) {
	call := model.ToolCall{
		Capability: "read_secret",
		Purpose:    "job-1",
		Resource:   "vault/item-1",
		Scope:      "vault",
	}
	catalog := mustCatalog(t, Rule{
		ID: "ask-secret", Version: "1.0", Precedence: 1,
		Decision: model.DecisionAsk,
		Match:    MatchCriteria{Capability: "read_secret"},
	})
	now := time.Now()
	cases := []struct {
		name  string
		grant *grant.Grant
		want  model.Decision
	}{
		{
			name: "matching capability purpose resource and scope",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-1",
				ScopeCeiling: []string{"vault"}, ExpiresAt: now.Add(time.Hour),
			},
			want: model.DecisionAllow,
		},
		{
			name: "unrelated capability does not authorize",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "credential_use", Purpose: "job-1", Resource: "vault/item-1",
				ScopeCeiling: []string{"vault"}, ExpiresAt: now.Add(time.Hour),
			},
			want: model.DecisionAsk,
		},
		{
			name: "cross-purpose grant does not authorize",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "read_secret", Purpose: "job-2", Resource: "vault/item-1",
				ScopeCeiling: []string{"vault"}, ExpiresAt: now.Add(time.Hour),
			},
			want: model.DecisionAsk,
		},
		{
			name: "cross-resource grant does not authorize",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-2",
				ScopeCeiling: []string{"vault"}, ExpiresAt: now.Add(time.Hour),
			},
			want: model.DecisionAsk,
		},
		{
			name: "broader scope is not implied",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-1",
				ScopeCeiling: []string{"session"}, ExpiresAt: now.Add(time.Hour),
			},
			want: model.DecisionAsk,
		},
		{
			name: "expired grant does not authorize",
			grant: &grant.Grant{
				Subject: "agent-1", Scope: grant.ScopeVault,
				Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-1",
				ScopeCeiling: []string{"vault"}, ExpiresAt: now.Add(-time.Second),
			},
			want: model.DecisionAsk,
		},
	}
	for _, tt := range cases {
		t.Run(tt.name, func(t *testing.T) {
			lookup := fakeLookup{subjects: map[string][]*grant.Grant{"agent-1": {tt.grant}}}
			got := catalog.EvaluateWithGrants("agent-1", call, model.DecisionAsk, lookup)
			if got.Decision != tt.want {
				t.Fatalf("decision = %q, want %q", got.Decision, tt.want)
			}
		})
	}
}

func TestGrantAuthorizesRejectsUnboundGrant(t *testing.T) {
	if (&grant.Grant{}).Authorizes("read_secret", "job-1", "item", "vault", time.Now()) {
		t.Fatal("unbound grant must not authorize a call")
	}
}

func TestEvaluateWithGrantsRejectsUnrelatedSubjectAndMissingScope(t *testing.T) {
	catalog := mustCatalog(t, Rule{
		ID: "ask", Version: "1.0", Precedence: 1,
		Decision: model.DecisionAsk,
		Match:    MatchCriteria{Capability: "read_secret"},
	})
	call := model.ToolCall{
		Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-1", Scope: "vault",
	}
	now := time.Now()
	standing := &grant.Grant{
		Subject: "agent-1", Scope: grant.ScopeVault,
		Capability: "read_secret", Purpose: "job-1", Resource: "vault/item-1",
		ScopeCeiling: []string{"*"}, ExpiresAt: now.Add(time.Hour),
	}

	if got := catalog.EvaluateWithGrants("agent-1", call, model.DecisionAsk,
		fakeLookup{subjects: map[string][]*grant.Grant{"agent-2": {standing}}}); got.Decision != model.DecisionAsk {
		t.Fatalf("unrelated subject decision = %q, want ask", got.Decision)
	}
	if got := catalog.EvaluateWithGrants("agent-1", call, model.DecisionAsk,
		fakeLookup{subjects: map[string][]*grant.Grant{"agent-1": {{
			Subject: "agent-2", Capability: standing.Capability, Purpose: standing.Purpose,
			Resource: standing.Resource, ScopeCeiling: standing.ScopeCeiling, ExpiresAt: standing.ExpiresAt,
		}}}}); got.Decision != model.DecisionAsk {
		t.Fatalf("mislabeled lookup grant decision = %q, want ask", got.Decision)
	}
	call.Scope = ""
	if got := catalog.EvaluateWithGrants("agent-1", call, model.DecisionAsk,
		fakeLookup{subjects: map[string][]*grant.Grant{"agent-1": {standing}}}); got.Decision != model.DecisionAsk {
		t.Fatalf("missing call scope decision = %q, want ask", got.Decision)
	}
}
