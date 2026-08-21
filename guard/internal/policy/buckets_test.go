package policy

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// bucketRule builds a rule for bucket-semantics tests.
func bucketRule(id string, precedence Precedence, decision model.Decision, match MatchCriteria) Rule {
	return Rule{
		ID:         RuleID(id),
		Version:    "1.0.0",
		Precedence: precedence,
		Decision:   decision,
		Match:      match,
		Reason:     "reason for " + id,
	}
}

func TestEvaluate_BucketSemantics(t *testing.T) {
	fsRead := MatchCriteria{Server: "filesystem", Tool: "read_file"}
	tests := []struct {
		name        string
		rules       []Rule
		call        model.ToolCall
		defaults    model.Decision
		want        model.Decision
		wantRule    RuleID
		wantMatched bool
	}{
		{
			name: "deny after allow wins regardless of position",
			rules: []Rule{
				bucketRule("allow-1", 10, model.DecisionAllow, fsRead),
				bucketRule("deny-1", 20, model.DecisionDeny, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionDeny,
			wantRule:    "deny-1",
			wantMatched: true,
		},
		{
			name: "allow after deny also denies",
			rules: []Rule{
				bucketRule("deny-1", 10, model.DecisionDeny, fsRead),
				bucketRule("allow-1", 20, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionDeny,
			wantRule:    "deny-1",
			wantMatched: true,
		},
		{
			name: "deny beats matching ask",
			rules: []Rule{
				bucketRule("ask-1", 10, model.DecisionAsk, fsRead),
				bucketRule("deny-1", 20, model.DecisionDeny, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionDeny,
			wantRule:    "deny-1",
			wantMatched: true,
		},
		{
			name: "allow wins when no deny matches",
			rules: []Rule{
				bucketRule("deny-1", 10, model.DecisionDeny, MatchCriteria{Server: "db"}),
				bucketRule("allow-1", 20, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionAllow,
			wantRule:    "allow-1",
			wantMatched: true,
		},
		{
			name: "matching ask is returned when no deny matched",
			rules: []Rule{
				bucketRule("ask-1", 10, model.DecisionAsk, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionAsk,
			wantRule:    "ask-1",
			wantMatched: true,
		},
		{
			name: "default applies when nothing matches",
			rules: []Rule{
				bucketRule("allow-1", 10, model.DecisionAllow, MatchCriteria{Server: "db"}),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionDeny,
			wantRule:    "",
			wantMatched: false,
		},
		{
			name:     "empty catalog uses default",
			rules:    nil,
			call:     model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults: model.DecisionAsk,
			want:     model.DecisionAsk,
		},
		{
			name: "require holds and allow matches",
			rules: []Rule{
				bucketRule("req-1", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
				bucketRule("allow-1", 20, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionAllow,
			wantRule:    "allow-1",
			wantMatched: true,
		},
		{
			name: "require failure denies even with matching allow",
			rules: []Rule{
				bucketRule("req-1", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
				bucketRule("allow-1", 20, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "write_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionDeny,
			wantRule:    "req-1",
			wantMatched: false,
		},
		{
			name: "all requires must hold",
			rules: []Rule{
				bucketRule("req-1", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
				bucketRule("req-2", 20, model.DecisionRequire, MatchCriteria{Server: "filesystem"}),
				bucketRule("allow-1", 30, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionAllow,
			wantRule:    "allow-1",
			wantMatched: true,
		},
		{
			name: "first failing require is reported",
			rules: []Rule{
				bucketRule("req-1", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
				bucketRule("req-2", 20, model.DecisionRequire, MatchCriteria{Server: "filesystem"}),
			},
			call:        model.ToolCall{Server: "db", Tool: "write_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionDeny,
			wantRule:    "req-1",
			wantMatched: false,
		},
		{
			name: "require holds with no allow falls back to default",
			rules: []Rule{
				bucketRule("req-1", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionDeny,
			wantRule:    "",
			wantMatched: false,
		},
		{
			name: "precedence orders within allow bucket",
			rules: []Rule{
				bucketRule("allow-1", 10, model.DecisionAllow, fsRead),
				bucketRule("allow-2", 20, model.DecisionAllow, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionDeny,
			want:        model.DecisionAllow,
			wantRule:    "allow-1",
			wantMatched: true,
		},
		{
			name: "first matching deny is reported",
			rules: []Rule{
				bucketRule("deny-1", 10, model.DecisionDeny, fsRead),
				bucketRule("deny-2", 20, model.DecisionDeny, fsRead),
			},
			call:        model.ToolCall{Server: "filesystem", Tool: "read_file"},
			defaults:    model.DecisionAllow,
			want:        model.DecisionDeny,
			wantRule:    "deny-1",
			wantMatched: true,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			c, err := NewCatalog(tt.rules, "1.0.0")
			if err != nil {
				t.Fatalf("NewCatalog: %v", err)
			}
			got := c.Evaluate(tt.call, tt.defaults)
			if got.Decision != tt.want {
				t.Errorf("decision = %q, want %q", got.Decision, tt.want)
			}
			if got.Matched != tt.wantMatched {
				t.Errorf("matched = %v, want %v", got.Matched, tt.wantMatched)
			}
			if tt.wantRule != "" {
				if got.Rule == nil || got.Rule.ID != tt.wantRule {
					t.Errorf("rule = %v, want %q", got.Rule, tt.wantRule)
				}
			} else if got.Rule != nil {
				t.Errorf("rule = %v, want nil", got.Rule)
			}
		})
	}
}

func TestEvaluate_DefensiveDeny(t *testing.T) {
	// A deny rule whose match criteria cannot be evaluated must resolve to
	// denied, never to "not denied".
	c, err := NewCatalog([]Rule{
		bucketRule("deny-cmd", 10, model.DecisionDeny, MatchCriteria{CommandContains: []string{"rm -rf"}}),
		bucketRule("allow-1", 20, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	call := model.ToolCall{Server: "filesystem", Tool: "execute", Args: 42}
	got := c.Evaluate(call, model.DecisionAllow)
	if got.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny", got.Decision)
	}
	if got.Rule == nil || got.Rule.ID != "deny-cmd" {
		t.Errorf("rule = %v, want deny-cmd", got.Rule)
	}
	if got.Matched {
		t.Error("defensive deny must report matched=false")
	}
	if got.Reason == "" {
		t.Error("defensive deny must explain why in the reason")
	}
	if got.Bucket != BucketDeny {
		t.Errorf("bucket = %q, want %q", got.Bucket, BucketDeny)
	}
}

func TestEvaluate_UnevaluableRequireDenies(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("req-cmd", 10, model.DecisionRequire, MatchCriteria{CommandContains: []string{"ls"}}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	call := model.ToolCall{Server: "filesystem", Tool: "execute", Args: 42}
	got := c.Evaluate(call, model.DecisionAllow)
	if got.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny", got.Decision)
	}
	if got.Rule == nil || got.Rule.ID != "req-cmd" {
		t.Errorf("rule = %v, want req-cmd", got.Rule)
	}
}

func TestEvaluate_UnevaluableAllowNeverGrants(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("allow-cmd", 10, model.DecisionAllow, MatchCriteria{CommandContains: []string{"ls"}}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	call := model.ToolCall{Server: "filesystem", Tool: "execute", Args: 42}
	got := c.Evaluate(call, model.DecisionDeny)
	if got.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want default deny", got.Decision)
	}
	if got.Matched {
		t.Error("unevaluable allow rule must not grant")
	}
	if got.Rule != nil {
		t.Errorf("rule = %v, want nil", got.Rule)
	}
}

func TestMerge_DenySurvivesComposition(t *testing.T) {
	allowCat, err := NewCatalog([]Rule{
		bucketRule("allow-fs", 10, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	denyCat, err := NewCatalog([]Rule{
		bucketRule("deny-fs", 10, model.DecisionDeny, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	call := model.ToolCall{Server: "filesystem", Tool: "read_file"}

	for _, tt := range []struct {
		name string
		a, b *Catalog
	}{
		{"allow then deny", allowCat, denyCat},
		{"deny then allow", denyCat, allowCat},
	} {
		t.Run(tt.name, func(t *testing.T) {
			merged, err := tt.a.Merge(tt.b)
			if err != nil {
				t.Fatalf("Merge: %v", err)
			}
			got := merged.Evaluate(call, model.DecisionAllow)
			if got.Decision != model.DecisionDeny {
				t.Errorf("merged decision = %q, want deny", got.Decision)
			}
			if got.Rule == nil || got.Rule.ID != "deny-fs" {
				t.Errorf("merged rule = %v, want deny-fs", got.Rule)
			}
		})
	}
}

func TestMerge_RequireSurvivesComposition(t *testing.T) {
	requireCat, err := NewCatalog([]Rule{
		bucketRule("req-tool", 10, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	allowCat, err := NewCatalog([]Rule{
		bucketRule("allow-fs", 10, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	merged, err := requireCat.Merge(allowCat)
	if err != nil {
		t.Fatalf("Merge: %v", err)
	}

	// A call satisfying the requirement is allowed by the merged catalog.
	ok := merged.Evaluate(model.ToolCall{Server: "filesystem", Tool: "read_file"}, model.DecisionDeny)
	if ok.Decision != model.DecisionAllow {
		t.Errorf("satisfying call decision = %q, want allow", ok.Decision)
	}
	// A call violating the requirement is denied even though allow-fs matches.
	bad := merged.Evaluate(model.ToolCall{Server: "filesystem", Tool: "write_file"}, model.DecisionAllow)
	if bad.Decision != model.DecisionDeny {
		t.Errorf("violating call decision = %q, want deny", bad.Decision)
	}
}

func TestMerge_PrecedenceRenumbered(t *testing.T) {
	a, err := NewCatalog([]Rule{
		bucketRule("a1", 10, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
		bucketRule("a2", 20, model.DecisionAllow, MatchCriteria{Tool: "read_file"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	b, err := NewCatalog([]Rule{
		bucketRule("b1", 10, model.DecisionAllow, MatchCriteria{Tool: "read_file"}),
		bucketRule("b2", 20, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}

	merged, err := a.Merge(b)
	if err != nil {
		t.Fatalf("Merge: %v", err)
	}
	if len(merged.Rules) != 4 {
		t.Fatalf("merged rules = %d, want 4", len(merged.Rules))
	}
	for i, r := range merged.Rules {
		if r.Precedence != Precedence(i+1) {
			t.Errorf("rules[%d].precedence = %d, want %d", i, r.Precedence, i+1)
		}
	}
	// Within-bucket ordering follows concatenation order: a1 matches first.
	got := merged.Evaluate(model.ToolCall{Server: "filesystem", Tool: "read_file"}, model.DecisionDeny)
	if got.Rule == nil || got.Rule.ID != "a1" {
		t.Errorf("rule = %v, want a1", got.Rule)
	}
}

func TestMerge_DuplicateIDRejected(t *testing.T) {
	a, err := NewCatalog([]Rule{
		bucketRule("dup", 10, model.DecisionAllow, MatchCriteria{Server: "a"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	b, err := NewCatalog([]Rule{
		bucketRule("dup", 10, model.DecisionDeny, MatchCriteria{Server: "b"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	if _, err := a.Merge(b); err == nil {
		t.Fatal("Merge with duplicate rule ID: expected error")
	}
}
