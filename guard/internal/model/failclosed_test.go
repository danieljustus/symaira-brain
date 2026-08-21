package model

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func TestFailureModeResolve(t *testing.T) {
	tests := []struct {
		name string
		mode FailureMode
		want Decision
	}{
		{"unset resolves to deny", FailureMode(""), DecisionDeny},
		{"unknown resolves to deny", FailureMode("bogus"), DecisionDeny},
		{"explicit deny", FailureModeDeny, DecisionDeny},
		{"explicit allow", FailureModeAllow, DecisionAllow},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.mode.Resolve(); got != tt.want {
				t.Errorf("Resolve() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestDecisionRequestExpired(t *testing.T) {
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	tests := []struct {
		name     string
		deadline time.Time
		at       time.Time
		want     bool
	}{
		{"zero deadline never expires", time.Time{}, now, false},
		{"future deadline not expired", now.Add(time.Hour), now, false},
		{"past deadline expired", now.Add(-time.Hour), now, true},
		{"exactly at deadline is expired", now, now, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := DecisionRequest{Deadline: tt.deadline}
			if got := req.Expired(tt.at); got != tt.want {
				t.Errorf("Expired() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestNewNoDecision_FailClosed(t *testing.T) {
	nd := NewNoDecision(FailureModeDeny, "transport error: connection refused")
	if nd.Control == nil {
		t.Fatal("Control must not be nil")
	}
	if nd.Control.Decision != DecisionDeny {
		t.Errorf("control decision = %q, want deny", nd.Control.Decision)
	}
	if nd.Control.Reason != "" {
		t.Errorf("control reason = %q, want empty (no distinguishing signal)", nd.Control.Reason)
	}
	if nd.Diagnostic != "transport error: connection refused" {
		t.Errorf("diagnostic = %q, want transport error", nd.Diagnostic)
	}
}

func TestNewNoDecision_UnsetFailureResolvesToDeny(t *testing.T) {
	// Every non-decision outcome must produce the same response shape as
	// an explicit deny even when the caller forgets the failure mode.
	nd := NewNoDecision(FailureMode(""), "timeout after 5s")
	if nd.Control.Decision != DecisionDeny {
		t.Errorf("control decision = %q, want deny", nd.Control.Decision)
	}
}

func TestNoDecisionSameShapeAsExplicitDeny(t *testing.T) {
	nd := NewNoDecision(FailureModeDeny, "timeout")
	explicit := &ControlResponse{Decision: DecisionDeny}

	ndJSON, err := json.Marshal(nd.Control)
	if err != nil {
		t.Fatalf("marshal no-decision control: %v", err)
	}
	explicitJSON, err := json.Marshal(explicit)
	if err != nil {
		t.Fatalf("marshal explicit deny: %v", err)
	}
	if string(ndJSON) != string(explicitJSON) {
		t.Errorf("no-decision control = %s, explicit deny = %s; must be byte-identical", ndJSON, explicitJSON)
	}
}

func TestNewNoDecision_ExplicitAllow(t *testing.T) {
	nd := NewNoDecision(FailureModeAllow, "configured to fail open")
	if nd.Control.Decision != DecisionAllow {
		t.Errorf("control decision = %q, want allow", nd.Control.Decision)
	}
}

func TestControlResponseCarriesNoDiagnostics(t *testing.T) {
	resp := &ControlResponse{Decision: DecisionDeny, Reason: "denied", RetryAfter: 5}
	data, err := json.Marshal(resp)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, forbidden := range []string{"trace", "rule_trace", "diagnostic", "failure_mode"} {
		if strings.Contains(string(data), forbidden) {
			t.Errorf("ControlResponse JSON contains %q: %s", forbidden, data)
		}
	}
}
