package policy

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

func validRule(id string, precedence Precedence) Rule {
	return Rule{
		ID:         RuleID(id),
		Version:    "1.0.0",
		Precedence: precedence,
		Decision:   model.DecisionAllow,
		Match:      MatchCriteria{Server: "filesystem", Tool: "read_file"},
		Reason:     "approved by test rule",
	}
}

func TestValidate_ValidCatalog(t *testing.T) {
	rules := []Rule{
		validRule("rule-1", 10),
		validRule("rule-2", 20),
	}
	c, err := NewCatalog(rules, "1.0.0")
	if err != nil {
		t.Fatalf("NewCatalog() error = %v", err)
	}
	if len(c.Rules) != 2 {
		t.Errorf("got %d rules, want 2", len(c.Rules))
	}
}

func TestValidate_EmptyID(t *testing.T) {
	_, err := NewCatalog([]Rule{
		{ID: "", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow},
	}, "1.0.0")
	if err == nil {
		t.Fatal("expected error for empty rule ID")
	}
}

func TestValidate_DuplicateID(t *testing.T) {
	_, err := NewCatalog([]Rule{
		validRule("dup", 10),
		validRule("dup", 20),
	}, "1.0.0")
	if err == nil {
		t.Fatal("expected error for duplicate rule ID")
	}
}

func TestValidate_InvalidDecision(t *testing.T) {
	_, err := NewCatalog([]Rule{
		{ID: "bad", Version: "1.0", Precedence: 10, Decision: "nope",
			Match: MatchCriteria{Server: "test"}},
	}, "1.0.0")
	if err == nil {
		t.Fatal("expected error for invalid decision")
	}
}

func TestValidate_EmptyMatch(t *testing.T) {
	_, err := NewCatalog([]Rule{
		{ID: "empty", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow},
	}, "1.0.0")
	if err == nil {
		t.Fatal("expected error for empty match criteria")
	}
}

func TestValidate_NonIncreasingPrecedence(t *testing.T) {
	_, err := NewCatalog([]Rule{
		validRule("a", 20),
		validRule("b", 10),
	}, "1.0.0")
	if err == nil {
		t.Fatal("expected error for decreasing precedence")
	}
}

func TestEvaluate_NoMatch(t *testing.T) {
	c, _ := NewCatalog([]Rule{validRule("r1", 10)}, "1.0.0")
	call := model.ToolCall{Server: "unknown", Tool: "unknown"}
	result := c.Evaluate(call, model.DecisionDeny)
	if result.Matched {
		t.Error("expected no match")
	}
	if result.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny", result.Decision)
	}
}

func TestEvaluate_MatchByServer(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow,
			Match: MatchCriteria{Server: "filesystem"}},
	}, "1.0.0")
	call := model.ToolCall{Server: "filesystem", Tool: "read_file"}
	result := c.Evaluate(call, model.DecisionDeny)
	if !result.Matched {
		t.Fatal("expected match")
	}
	if result.Decision != model.DecisionAllow {
		t.Errorf("decision = %q, want allow", result.Decision)
	}
}

func TestEvaluate_MatchByTool(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
			Match: MatchCriteria{Tool: "shell_exec"}},
	}, "1.0.0")
	call := model.ToolCall{Server: "any", Tool: "shell_exec"}
	result := c.Evaluate(call, model.DecisionAllow)
	if !result.Matched {
		t.Fatal("expected match")
	}
	if result.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny", result.Decision)
	}
}

func TestEvaluate_PrecedenceOrdering(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "high", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
			Match: MatchCriteria{Server: "db"}},
		{ID: "low", Version: "1.0", Precedence: 20, Decision: model.DecisionAllow,
			Match: MatchCriteria{Server: "db"}},
	}, "1.0.0")
	call := model.ToolCall{Server: "db", Tool: "query"}
	result := c.Evaluate(call, model.DecisionAllow)
	if !result.Matched {
		t.Fatal("expected match")
	}
	// Higher precedence (lower number) should win
	if result.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny (higher precedence should win)", result.Decision)
	}
	if result.Rule == nil || result.Rule.ID != "high" {
		t.Errorf("matched rule = %v, want high", result.Rule)
	}
}

func TestEvaluate_CommandContains(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
			Match: MatchCriteria{CommandContains: []string{"rm -rf", "curl | sh"}}},
	}, "1.0.0")
	call := model.ToolCall{Server: "shell", Tool: "execute", Args: "rm -rf /"}
	result := c.Evaluate(call, model.DecisionAllow)
	if !result.Matched {
		t.Fatal("expected match for command containing 'rm -rf'")
	}
	if result.Decision != model.DecisionDeny {
		t.Errorf("decision = %q, want deny", result.Decision)
	}
}

func TestEvaluate_CommandContainsNoMatch(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
			Match: MatchCriteria{CommandContains: []string{"rm -rf"}}},
	}, "1.0.0")
	call := model.ToolCall{Server: "shell", Tool: "execute", Args: "ls -la"}
	result := c.Evaluate(call, model.DecisionAllow)
	if result.Matched {
		t.Error("expected no match for safe command")
	}
}

func TestEvaluate_MatchByCapability(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionAsk,
			Match: MatchCriteria{Capability: "read_secret"}},
	}, "1.0.0")
	call := model.ToolCall{Server: "vault", Tool: "get", Capability: "read_secret"}
	result := c.Evaluate(call, model.DecisionAllow)
	if !result.Matched {
		t.Fatal("expected match")
	}
	if result.Decision != model.DecisionAsk {
		t.Errorf("decision = %q, want ask", result.Decision)
	}
}

func TestResult_StructuredMetadata(t *testing.T) {
	c, _ := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionAllow,
			Match: MatchCriteria{Server: "fs"}, Reason: "explicit allow for fs"},
	}, "1.0.0")
	call := model.ToolCall{Server: "fs", Tool: "read"}
	result := c.Evaluate(call, model.DecisionDeny)
	if !result.Matched {
		t.Fatal("expected match")
	}
	if result.Reason != "explicit allow for fs" {
		t.Errorf("reason = %q, want explicit allow for fs", result.Reason)
	}
	if result.Precedence != 10 {
		t.Errorf("precedence = %d, want 10", result.Precedence)
	}
}
