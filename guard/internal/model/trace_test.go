package model

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestEvaluationRuleTraceOmittedWhenOff(t *testing.T) {
	// With tracing off, the serialized event must be byte-identical to
	// today's output: the rule_trace field is absent.
	evt := ActionEvent{
		ID:        EventID(SourceProxy, 1),
		SchemaVer: SchemaVersion,
		Source:    SourceProxy,
		State:     ActionRequested,
		Timestamp: "2026-08-06T12:00:00Z",
		Agent:     AgentIdentity{AgentID: "test-agent"},
		Call:      ToolCall{Server: "filesystem", Tool: "read_file"},
		Evaluation: &Evaluation{
			MatchedRule:   "allow-1",
			Decision:      DecisionAllow,
			Reason:        "approved by test rule",
			MarginalCheck: false,
		},
	}
	data, err := json.Marshal(evt)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if strings.Contains(string(data), "rule_trace") {
		t.Errorf("rule_trace must be omitted when tracing is off: %s", data)
	}

	// An empty trace slice must also serialize as absent.
	empty := &Evaluation{MatchedRule: "r1", Decision: DecisionDeny, RuleTrace: []RuleTraceEntry{}}
	data, err = json.Marshal(empty)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if strings.Contains(string(data), "rule_trace") {
		t.Errorf("rule_trace must be omitted for an empty trace: %s", data)
	}
}

func TestEvaluationRuleTraceRoundTrip(t *testing.T) {
	eval := &Evaluation{
		MatchedRule: "allow-1",
		Decision:    DecisionAllow,
		Reason:      "approved by test rule",
		RuleTrace: []RuleTraceEntry{
			{RuleID: "deny-1", Matched: false, Decision: DecisionDeny, Bucket: "deny"},
			{RuleID: "req-1", Matched: true, Decision: DecisionRequire, Bucket: "require"},
			{RuleID: "allow-1", Matched: true, Decision: DecisionAllow, Bucket: "allow"},
		},
	}
	data, err := json.Marshal(eval)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if !strings.Contains(string(data), "rule_trace") {
		t.Fatalf("rule_trace missing from serialized evaluation: %s", data)
	}

	var back Evaluation
	if err := json.Unmarshal(data, &back); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(back.RuleTrace) != 3 {
		t.Fatalf("rule trace length = %d, want 3", len(back.RuleTrace))
	}
	if back.RuleTrace[1] != (RuleTraceEntry{RuleID: "req-1", Matched: true, Decision: DecisionRequire, Bucket: "require"}) {
		t.Errorf("rule trace[1] = %+v, want req-1 matched require", back.RuleTrace[1])
	}
}
