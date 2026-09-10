package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/guard/internal/model"
)

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
