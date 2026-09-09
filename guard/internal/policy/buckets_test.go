package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/guard/internal/model"
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
