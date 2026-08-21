package policy

import (
	"github.com/danieljustus/symaira-guard/internal/model"
	"github.com/danieljustus/symaira-guard/internal/sequence"
)

// SequenceRule is a stateful rule that detects repetitive tool-call patterns
// across the model.ActionEvent stream, complementing the stateless per-call
// rules in rule.go. It wraps a sequence.Detector and maps its Evaluation
// onto the policy Result shape.
//
// Recoverability: a deny from this rule is always recoverable per the
// sequence package contract — the rule refuses the individual call and never
// terminates the session, and the reason carries the "sequence: " prefix.
//
// The rule is inert (always allows) unless sequence.Config.Enabled is set,
// matching the opt-in [sequence] TOML section (off by default).
type SequenceRule struct {
	detector *sequence.Detector
}

// NewSequenceRule constructs a SequenceRule from a sequence.Config.
func NewSequenceRule(cfg sequence.Config) *SequenceRule {
	return &SequenceRule{detector: sequence.NewDetector(cfg)}
}

// Evaluate feeds one event through the sequence detector and returns the
// resulting policy Result. A blocked call yields DecisionDeny with
// Matched=true and the detector's reason; anything else yields
// DecisionAllow with Matched=false.
func (r *SequenceRule) Evaluate(ev model.ActionEvent) Result {
	res := r.detector.Evaluate(ev)
	return Result{
		Decision: res.Decision,
		Reason:   res.Reason,
		Matched:  res.Decision == model.DecisionDeny,
	}
}
