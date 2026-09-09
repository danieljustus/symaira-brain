package model

import (
	"fmt"
	"strings"
)

// ValidateFailureMode rejects an unrecognized failure mode. An empty mode is
// valid and deliberately means the fail-closed default (deny).
func ValidateFailureMode(f FailureMode) error {
	switch f {
	case "", FailureModeDeny, FailureModeAllow:
		return nil
	default:
		return fmt.Errorf("model: unknown failure mode %q", f)
	}
}

// ValidateToolCall checks the identity fields needed to make a policy
// decision. Arguments are intentionally opaque: policy matchers validate the
// shapes they consume, rather than the model layer guessing a schema.
func ValidateToolCall(call ToolCall) error {
	if strings.TrimSpace(call.Server) == "" {
		return fmt.Errorf("model: call server is required")
	}
	if strings.TrimSpace(call.Tool) == "" {
		return fmt.Errorf("model: call tool is required")
	}
	return nil
}

// ValidateDecisionRequest checks the request envelope without evaluating it.
// FailureModeDeny's zero value remains valid so malformed/unset failure modes
// can still be handled by a fail-closed caller.
func ValidateDecisionRequest(r DecisionRequest) error {
	if err := ValidateToolCall(r.Call); err != nil {
		return fmt.Errorf("model: invalid decision request: %w", err)
	}
	if err := ValidateFailureMode(r.Failure); err != nil {
		return fmt.Errorf("model: invalid decision request: %w", err)
	}
	return nil
}

// ValidateControlResponse checks the small control-plane response. Diagnostic
// fields do not belong in this value; JSON shape enforcement is left to the
// wire decoder because callers may use additional transport-level checks.
func ValidateControlResponse(r ControlResponse) error {
	if err := ValidateDecision(r.Decision); err != nil {
		return fmt.Errorf("model: invalid control response: %w", err)
	}
	if r.RetryAfter < 0 {
		return fmt.Errorf("model: invalid control response: retry_after must not be negative")
	}
	return nil
}

// ValidateActionEvent checks the stable fields required for an audit-safe
// event. It intentionally does not validate redacted argument payloads.
func ValidateActionEvent(e ActionEvent) error {
	if strings.TrimSpace(e.ID) == "" {
		return fmt.Errorf("model: event ID is required")
	}
	if e.SchemaVer != SchemaVersion {
		return fmt.Errorf("model: unsupported schema version %d", e.SchemaVer)
	}
	if err := ValidateSource(e.Source); err != nil {
		return err
	}
	if err := ValidateState(e.State); err != nil {
		return err
	}
	if strings.TrimSpace(e.Timestamp) == "" {
		return fmt.Errorf("model: event timestamp is required")
	}
	if strings.TrimSpace(e.Agent.AgentID) == "" {
		return fmt.Errorf("model: event agent ID is required")
	}
	if err := ValidateToolCall(e.Call); err != nil {
		return fmt.Errorf("model: invalid event call: %w", err)
	}
	if e.ControlResp != nil {
		if err := ValidateControlResponse(*e.ControlResp); err != nil {
			return err
		}
	}
	if e.Evaluation != nil {
		if err := ValidateDecision(e.Evaluation.Decision); err != nil {
			return fmt.Errorf("model: invalid evaluation: %w", err)
		}
	}
	return nil
}
