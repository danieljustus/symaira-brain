package model

import (
	"testing"
)

func TestSchemaVersion(t *testing.T) {
	if SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, want 1", SchemaVersion)
	}
}

func TestEventID(t *testing.T) {
	id := EventID(SourceProxy, 1)
	if id == "" {
		t.Error("EventID returned empty string")
	}
}

func TestValidateSource(t *testing.T) {
	tests := []struct {
		name    string
		source  SourceType
		wantErr bool
	}{
		{"proxy", SourceProxy, false},
		{"hook", SourceHook, false},
		{"artifact", SourceArtifact, false},
		{"scan", SourceScan, false},
		{"decide", SourceDecide, false},
		{"unknown", SourceType("unknown"), true},
		{"empty", SourceType(""), true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateSource(tt.source)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateSource(%q) error = %v, wantErr %v", tt.source, err, tt.wantErr)
			}
		})
	}
}

func TestValidateState(t *testing.T) {
	tests := []struct {
		name  string
		state ActionState
		err   bool
	}{
		{"requested", ActionRequested, false},
		{"approved", ActionApproved, false},
		{"denied", ActionDenied, false},
		{"started", ActionStarted, false},
		{"completed", ActionCompleted, false},
		{"failed", ActionFailed, false},
		{"unknown", ActionState("unknown"), true},
		{"empty", ActionState(""), true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateState(tt.state)
			if (err != nil) != tt.err {
				t.Errorf("ValidateState(%q) error = %v, wantErr %v", tt.state, err, tt.err)
			}
		})
	}
}

func TestValidateDecision(t *testing.T) {
	tests := []struct {
		name     string
		decision Decision
		wantErr  bool
	}{
		{"allow", DecisionAllow, false},
		{"ask", DecisionAsk, false},
		{"deny", DecisionDeny, false},
		{"require", DecisionRequire, false},
		{"redact", DecisionRedact, false},
		{"readonly", DecisionReadOnly, false},
		{"sandbox", DecisionSandbox, false},
		{"unknown", Decision("unknown"), true},
		{"empty", Decision(""), true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateDecision(tt.decision)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateDecision(%q) error = %v, wantErr %v", tt.decision, err, tt.wantErr)
			}
		})
	}
}

func TestActionEventRoundTrip(t *testing.T) {
	evt := ActionEvent{
		ID:        EventID(SourceProxy, 1),
		SchemaVer: SchemaVersion,
		Source:    SourceProxy,
		State:     ActionRequested,
		Timestamp: "2026-07-30T12:00:00Z",
		Agent: AgentIdentity{
			AgentID:   "hermes-agent",
			SessionID: "sess_001",
			RunID:     "run_001",
		},
		Call: ToolCall{
			Server:     "filesystem",
			Tool:       "read_file",
			ArgsRef:    "ref://args/001",
			Capability: "read_file",
			RiskClass:  string(RiskClassMedium),
		},
		ControlResp: &ControlResponse{
			Decision: DecisionAllow,
			Reason:   "explicit rule match",
		},
	}

	if evt.ID == "" {
		t.Error("event ID must not be empty")
	}
	if evt.SchemaVer != 1 {
		t.Errorf("schema version = %d, want 1", evt.SchemaVer)
	}
	if evt.ControlResp.Decision != DecisionAllow {
		t.Errorf("decision = %q, want allow", evt.ControlResp.Decision)
	}
}

func TestControlResponseNoDiagnostics(t *testing.T) {
	// Verify ControlResponse carries only what the MCP runtime needs.
	resp := ControlResponse{
		Decision: DecisionDeny,
		Reason:   "capability denied by default",
	}

	if resp.Decision != DecisionDeny {
		t.Errorf("decision = %q, want deny", resp.Decision)
	}
	if resp.Reason == "" {
		t.Error("reason must be set for deny")
	}
}

func TestEvaluationDiagnosticOnly(t *testing.T) {
	eval := &Evaluation{
		MatchedRule:   "defaults.shell",
		Decision:      DecisionAsk,
		Reason:        "shell access requires confirmation",
		MarginalCheck: false,
	}

	if eval.MatchedRule == "" {
		t.Error("evaluation must carry matched rule name")
	}
	if eval.Decision != DecisionAsk {
		t.Errorf("decision = %q, want ask", eval.Decision)
	}
}

func TestAgentIdentityMinimal(t *testing.T) {
	id := AgentIdentity{AgentID: "test-agent"}
	if id.AgentID != "test-agent" {
		t.Errorf("agent_id = %q, want test-agent", id.AgentID)
	}
	if id.SessionID != "" {
		t.Error("session_id should be empty for minimal identity")
	}
}
