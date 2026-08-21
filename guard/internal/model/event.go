// Package model defines the versioned, redaction-safe event contract for
// symguard's policy evaluation, approval flow, MCP proxy, and audit log.
//
// Schema version: 1 (increment on any backward-incompatible change to any type
// in this package). Additive changes (new source types, new decision values,
// new optional fields) extend the schema but do not change the version.
//
// # Core invariant
//
// Every event has a stable ID and enough provenance to correlate policy
// evaluation, approval, execution, and audit records. Sensitive arguments,
// tool output, paths, and credentials are redacted or represented by safe
// references before leaving the control boundary.
//
// # Control / Diagnostic separation
//
// ControlResponse contains only what the MCP proxy or agent runtime needs:
// allow, deny, ask, or a passthrough signal. All diagnostic context (rule
// expression, matched rule, evaluation trace) belongs in the ActionEvent's
// Evaluation field and is written to stderr or the audit sink, never to
// stdout in MCP transport mode. The optional per-rule trace
// (Evaluation.RuleTrace) follows the same rule: it is diagnostic only,
// populated only when the policy engine is explicitly asked to trace, and
// never appears in ControlResponse.
//
// # Fail-closed decision contract
//
// DecisionRequest carries the two fields that make an external decision
// call fail closed by construction. FailureMode is a typed failure mode
// whose zero value and any unrecognized value resolve to deny — matching
// the fail-closed convention documented in internal/config, where an
// unrecognized decision is rejected rather than implicitly allowed; only
// an explicitly configured FailureModeAllow resolves to allow. Deadline
// bounds how long a decision may take: once it passes, the request is
// expired and the outcome is a non-decision, never a stale decision.
//
// Every non-decision outcome — deadline expiry, transport error, malformed
// response — is produced by the single NewNoDecision constructor. Its
// Control is an explicit deny, indistinguishable in shape from a policy
// deny; the reason a decision was not produced lives only in the
// Diagnostic field, so callers cannot accidentally distinguish "denied"
// from "unavailable" in a permissive direction.
package model

import (
	"fmt"
	"time"
)

// SchemaVersion is the current version of the ActionEvent schema.
// Increment on any backward-incompatible change; additive changes are
// compatible and do not change this constant.
const SchemaVersion = 1

// SourceType identifies where an ActionEvent originated.
type SourceType string

const (
	SourceProxy    SourceType = "proxy"    // MCP proxy intercept
	SourceHook     SourceType = "hook"     // Pre/post execution hook
	SourceArtifact SourceType = "artifact" // Declared tool or script output
	SourceScan     SourceType = "scan"     // Offline tool-scanning report
	SourceDecide   SourceType = "decide"   // External classifier decision interface (symguard decide)
)

// ActionState describes the lifecycle phase of a tool call.
type ActionState string

const (
	ActionRequested ActionState = "requested"
	ActionApproved  ActionState = "approved"
	ActionDenied    ActionState = "denied"
	ActionStarted   ActionState = "started"
	ActionCompleted ActionState = "completed"
	ActionFailed    ActionState = "failed"
)

// RiskClass is a static classification assigned to a capability name.
type RiskClass string

const (
	RiskClassLow      RiskClass = "low"
	RiskClassMedium   RiskClass = "medium"
	RiskClassHigh     RiskClass = "high"
	RiskClassCritical RiskClass = "critical"
)

// Decision is the policy outcome for a tool call.
type Decision string

const (
	DecisionAllow    Decision = "allow"
	DecisionAsk      Decision = "ask"
	DecisionDeny     Decision = "deny"
	DecisionRequire  Decision = "require" // rule that must hold; see internal/policy bucket evaluation
	DecisionRedact   Decision = "redact"
	DecisionReadOnly Decision = "readonly"
	DecisionSandbox  Decision = "sandbox"
)

// FailureMode is a typed failure mode for decision requests.
// The zero value and any unrecognized value resolve to deny (fail
// closed); only an explicitly configured FailureModeAllow resolves to
// allow. See the package doc's "Fail-closed decision contract" section.
type FailureMode string

const (
	// FailureModeDeny is the zero value: when a decision cannot be
	// produced, the outcome is deny.
	FailureModeDeny FailureMode = "deny"
	// FailureModeAllow is an explicit, deliberate opt-in to fail open.
	FailureModeAllow FailureMode = "allow"
)

// Resolve returns the decision for a failure. Unset and unrecognized
// values resolve to deny; only an explicitly configured allow resolves
// to allow.
func (f FailureMode) Resolve() Decision {
	if f == FailureModeAllow {
		return DecisionAllow
	}
	return DecisionDeny
}

// AgentIdentity identifies the calling agent or client.
type AgentIdentity struct {
	AgentID   string `json:"agent_id"`
	SessionID string `json:"session_id,omitempty"`
	RunID     string `json:"run_id,omitempty"`
}

// ClientIdentity identifies the human or system that configured the agent.
type ClientIdentity struct {
	UserID string `json:"user_id,omitempty"`
	Host   string `json:"host,omitempty"`
}

// ToolCall captures an MCP tool request.
type ToolCall struct {
	Server     string `json:"server"`
	Tool       string `json:"tool"`
	Args       any    `json:"args,omitempty"`       // redacted before leaving the control boundary
	Result     any    `json:"result,omitempty"`     // redacted before leaving the control boundary
	ArgsRef    string `json:"args_ref,omitempty"`   // safe reference to the raw args
	ResultRef  string `json:"result_ref,omitempty"` // safe reference to the raw result
	Capability string `json:"capability,omitempty"` // risk-class key (e.g. "shell", "read_secret")
	RiskClass  string `json:"risk_class,omitempty"` // resolved risk classification
	Error      string `json:"error,omitempty"`      // non-empty when the call failed
}

// DecisionRequest asks for a policy decision on a tool call and carries
// the fail-closed contract for the request.
type DecisionRequest struct {
	Call     ToolCall    `json:"call"`
	Failure  FailureMode `json:"failure_mode,omitempty"`
	Deadline time.Time   `json:"deadline,omitempty"` // RFC 3339; zero means no deadline
}

// Expired reports whether the deadline has passed at the given time.
// A zero Deadline never expires. When a request expires, its outcome is a
// non-decision: the transport MUST answer with NewNoDecision (fail
// closed), never with a stale decision.
func (r DecisionRequest) Expired(now time.Time) bool {
	return !r.Deadline.IsZero() && !now.Before(r.Deadline)
}

// RuleTraceEntry records one rule's evaluation for the optional per-rule
// trace. Diagnostic only; populated exclusively by the policy engine when
// tracing is explicitly enabled. Bucket is one of "deny", "allow", or
// "require".
type RuleTraceEntry struct {
	RuleID   string   `json:"rule_id"`
	Matched  bool     `json:"matched"`
	Decision Decision `json:"decision"`
	Bucket   string   `json:"bucket"`
}

// Evaluation records the policy engine's reasoning (diagnostic only).
type Evaluation struct {
	MatchedRule   string           `json:"matched_rule,omitempty"`
	Decision      Decision         `json:"decision"`
	Reason        string           `json:"reason,omitempty"`
	MarginalCheck bool             `json:"marginal_check,omitempty"`
	RuleTrace     []RuleTraceEntry `json:"rule_trace,omitempty"` // diagnostic only; empty unless tracing is enabled
}

// ActionEvent is the versioned, immutable record of one policy-relevant action.
type ActionEvent struct {
	ID          string           `json:"id"`
	SchemaVer   int              `json:"schema_version"`
	Source      SourceType       `json:"source"`
	PrevEventID string           `json:"prev_event_id,omitempty"` // chain for audit log
	Agent       AgentIdentity    `json:"agent"`
	Client      ClientIdentity   `json:"client,omitempty"`
	Call        ToolCall         `json:"call"`
	State       ActionState      `json:"state"`
	Evaluation  *Evaluation      `json:"evaluation,omitempty"` // diagnostic only
	ControlResp *ControlResponse `json:"control_response,omitempty"`
	Timestamp   string           `json:"timestamp"` // RFC 3339
}

// ControlResponse is the immediate action the runtime should take.
// It contains zero diagnostic context.
type ControlResponse struct {
	Decision   Decision `json:"decision"`
	Reason     string   `json:"reason,omitempty"`      // user-facing, safe
	RetryAfter int      `json:"retry_after,omitempty"` // seconds
}

// NoDecision is the outcome of a decision request that did not produce a
// decision: deadline expiry, transport error, or malformed response.
// Control is always an explicit deny — indistinguishable in shape from a
// policy deny — so a caller cannot accidentally distinguish "denied"
// from "unavailable" in a permissive direction. Diagnostic explains why
// the decision was not produced and belongs in the ActionEvent's
// Evaluation field (the diagnostic side of the control/diagnostic
// separation), never in Control.
type NoDecision struct {
	Control    *ControlResponse
	Diagnostic string
}

// NewNoDecision is the single constructor for every non-decision outcome
// of a decision request. Every error path — deadline expiry, transport
// error, malformed response — MUST use it, so an outage and a deny
// produce the same observable outcome. The failure mode resolves the
// control decision: the default FailureModeDeny yields an explicit deny;
// only an explicitly configured FailureModeAllow yields an allow. The
// Control's Reason is left empty so the control path carries no signal
// that distinguishes unavailability from a deny.
func NewNoDecision(failure FailureMode, diagnostic string) NoDecision {
	return NoDecision{
		Control:    &ControlResponse{Decision: failure.Resolve()},
		Diagnostic: diagnostic,
	}
}

// EventID generates a stable, locally unique event ID.
// Format: "evt_<schema>_<source>_<counter>".
func EventID(source SourceType, counter int64) string {
	return fmt.Sprintf("evt_%d_%s_%d", SchemaVersion, source, counter)
}

// ValidateSource returns an error if the source type is unrecognized.
func ValidateSource(s SourceType) error {
	switch s {
	case SourceProxy, SourceHook, SourceArtifact, SourceScan, SourceDecide:
		return nil
	}
	return fmt.Errorf("model: unknown source type %q", s)
}

// ValidateState returns an error if the action state is unrecognized.
func ValidateState(s ActionState) error {
	switch s {
	case ActionRequested, ActionApproved, ActionDenied,
		ActionStarted, ActionCompleted, ActionFailed:
		return nil
	}
	return fmt.Errorf("model: unknown action state %q", s)
}

// ValidateDecision returns an error if the decision value is unrecognized.
func ValidateDecision(d Decision) error {
	switch d {
	case DecisionAllow, DecisionAsk, DecisionDeny, DecisionRequire,
		DecisionRedact, DecisionReadOnly, DecisionSandbox:
		return nil
	}
	return fmt.Errorf("model: unknown decision %q", d)
}
