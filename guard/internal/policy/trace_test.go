package policy

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

func TestEvaluate_TraceOffByDefault(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("deny-1", 10, model.DecisionDeny, MatchCriteria{Server: "db"}),
		bucketRule("allow-1", 20, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	got := c.Evaluate(model.ToolCall{Server: "db", Tool: "query"}, model.DecisionAllow)
	if got.Trace != nil {
		t.Fatalf("Trace = %v, want nil when tracing is off", got.Trace)
	}
}

func TestEvaluate_TraceListsEveryRuleOnce(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("deny-1", 10, model.DecisionDeny, MatchCriteria{Server: "db"}),
		bucketRule("req-1", 20, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
		bucketRule("allow-1", 30, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}

	tests := []struct {
		name string
		call model.ToolCall
	}{
		{"deny wins", model.ToolCall{Server: "db", Tool: "read_file"}},
		{"require fails", model.ToolCall{Server: "db", Tool: "write_file"}},
		{"allow wins", model.ToolCall{Server: "filesystem", Tool: "read_file"}},
		{"default applies", model.ToolCall{Server: "other", Tool: "other"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := c.EvaluateOpts(tt.call, model.DecisionDeny, Options{Trace: true})
			if len(got.Trace) != 3 {
				t.Fatalf("trace length = %d, want 3", len(got.Trace))
			}
			seen := map[string]bool{}
			for _, e := range got.Trace {
				if seen[e.RuleID] {
					t.Errorf("rule %q traced more than once", e.RuleID)
				}
				seen[e.RuleID] = true
			}
			for _, wantID := range []string{"deny-1", "req-1", "allow-1"} {
				if !seen[wantID] {
					t.Errorf("rule %q missing from trace", wantID)
				}
			}
		})
	}
}

func TestEvaluate_TraceMatchedStates(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("deny-1", 10, model.DecisionDeny, MatchCriteria{Server: "db"}),
		bucketRule("req-1", 20, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
		bucketRule("allow-1", 30, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	got := c.EvaluateOpts(model.ToolCall{Server: "db", Tool: "read_file"}, model.DecisionDeny, Options{Trace: true})
	want := []model.RuleTraceEntry{
		{RuleID: "deny-1", Matched: true, Decision: model.DecisionDeny, Bucket: string(BucketDeny)},
		{RuleID: "req-1", Matched: true, Decision: model.DecisionRequire, Bucket: string(BucketRequire)},
		{RuleID: "allow-1", Matched: false, Decision: model.DecisionAllow, Bucket: string(BucketAllow)},
	}
	if len(got.Trace) != len(want) {
		t.Fatalf("trace length = %d, want %d", len(got.Trace), len(want))
	}
	for i := range want {
		if got.Trace[i] != want[i] {
			t.Errorf("trace[%d] = %+v, want %+v", i, got.Trace[i], want[i])
		}
	}
}

func TestEvaluate_TraceDoesNotChangeDecision(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("deny-1", 10, model.DecisionDeny, MatchCriteria{Server: "db"}),
		bucketRule("req-1", 20, model.DecisionRequire, MatchCriteria{Tool: "read_file"}),
		bucketRule("allow-1", 30, model.DecisionAllow, MatchCriteria{Server: "filesystem"}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	call := model.ToolCall{Server: "db", Tool: "query"}
	plain := c.Evaluate(call, model.DecisionAllow)
	traced := c.EvaluateOpts(call, model.DecisionAllow, Options{Trace: true})
	if plain.Decision != traced.Decision || plain.Matched != traced.Matched {
		t.Errorf("tracing changed the outcome: plain=%+v traced=%+v", plain, traced)
	}
	if plain.Rule == nil || traced.Rule == nil || plain.Rule.ID != traced.Rule.ID {
		t.Errorf("tracing changed the deciding rule: plain=%v traced=%v", plain.Rule, traced.Rule)
	}
}

func TestEvaluate_TraceRecordsUnevaluableRule(t *testing.T) {
	c, err := NewCatalog([]Rule{
		bucketRule("deny-cmd", 10, model.DecisionDeny, MatchCriteria{CommandContains: []string{"rm"}}),
	}, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog: %v", err)
	}
	got := c.EvaluateOpts(model.ToolCall{Server: "s", Tool: "t", Args: 42}, model.DecisionAllow, Options{Trace: true})
	if len(got.Trace) != 1 {
		t.Fatalf("trace length = %d, want 1", len(got.Trace))
	}
	if got.Trace[0].RuleID != "deny-cmd" || got.Trace[0].Matched {
		t.Errorf("trace[0] = %+v, want deny-cmd unmatched", got.Trace[0])
	}
	if got.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want defensive deny", got.Decision)
	}
}
