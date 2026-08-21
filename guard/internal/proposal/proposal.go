// Package proposal defines the persisted request type for durable policy
// changes in symguard's approval layer: a Proposal is a human-facing,
// expiring request to set or delete a config.Rule, with its own lifecycle
// (pending, applied, rejected, expired).
//
// # Separation from approval
//
// approval.Request is deliberately ephemeral — "the request IS the state":
// one decision per tool call, nothing persisted. A Proposal is the
// opposite: it is a persisted, JSON-serializable object, and it is the
// only thing allowed to mutate the durable rule set. A proposal is always
// applied by an explicit human decision and never auto-applies on a
// timeout: when its expiry passes while unanswered it is expired, never
// granted. An agent therefore has a path from a deny to a durable rule —
// request, then let the human apply it — but can never widen its own
// denylist (or any rule set) without that human apply step.
//
// # Ambiguity semantics
//
// A delete action may omit part or all of its identity (its Match
// criteria) and resolve against the current rule set at apply time:
// a unique match applies, multiple matches return the candidate list in
// an AmbiguousError, and no match is a NoMatchError. A delete with no
// criteria at all matches every rule, so it can only ever apply when the
// rule set is a singleton — otherwise it is ambiguous or a no-match —
// and the audit record always identifies the rule it removed.
//
// # Audit
//
// Applying a proposal emits an audit.ProposalApplied record through the
// same sink pattern used for approval decisions, so a rule's provenance
// (which proposal produced it, and which human applied it) lands in the
// audit hash chain.
package proposal

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/danieljustus/symaira-guard/internal/audit"
	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/model"
)

// State is the lifecycle state of a Proposal.
type State string

const (
	// StatePending means the proposal is awaiting a human decision.
	StatePending State = "pending"
	// StateApplied means a human applied the proposal and its action has
	// been merged into the rule set.
	StateApplied State = "applied"
	// StateRejected means a human declined the proposal.
	StateRejected State = "rejected"
	// StateExpired means the proposal's expiry passed before a human
	// decision. An expired proposal is never applied.
	StateExpired State = "expired"
)

// Action is a typed request to mutate the durable rule set. Exactly one
// of Set or Delete must be set.
type Action struct {
	// Set upserts a rule: an existing rule with an identical match is
	// replaced in place (idempotent when the decision is unchanged),
	// otherwise the rule is appended.
	Set *config.Rule `json:"set,omitempty"`
	// Delete removes a rule identified by its match criteria, which may
	// be partial or entirely omitted (see the package doc ambiguity
	// rules).
	Delete *Delete `json:"delete,omitempty"`
}

// Delete identifies a rule to remove from the durable rule set. Match
// carries the identity criteria; when it is partial — or entirely
// omitted — identity resolves against the current rule set at apply
// time.
type Delete struct {
	Match config.RuleMatch `json:"match"`
}

// Proposal is a persisted request for a durable policy change, with its
// own lifecycle. Unlike approval.Request it is not tied to a single tool
// call: it survives, expires, and is the only object that can mutate the
// durable rule set — always by an explicit human decision.
type Proposal struct {
	ID          string              `json:"id"`
	RequestedBy model.AgentIdentity `json:"requested_by"`
	Reason      string              `json:"reason"`
	Action      Action              `json:"action"`
	State       State               `json:"state"`
	CreatedAt   time.Time           `json:"created_at"`
	ExpiresAt   time.Time           `json:"expires_at"`

	AppliedAt      time.Time `json:"applied_at,omitempty"`
	RejectedBy     string    `json:"rejected_by,omitempty"`
	RejectedReason string    `json:"rejected_reason,omitempty"`
	RejectedAt     time.Time `json:"rejected_at,omitempty"`
}

// Sink receives the audit record emitted when a proposal is applied. It
// is the proposal counterpart of the audit destination used for approval
// decisions: a concrete implementation appends the record to the hash
// chain (audit.HashEntry) and re-anchors it (audit.WriteCheckpoint) so
// the rule's provenance is in the chain.
type Sink interface {
	AppendProposalApplied(audit.ProposalApplied) error
}

// AmbiguousError reports that a delete action's criteria match more than
// one rule. Candidates holds the matching rules so the caller can
// present them to the human for disambiguation.
type AmbiguousError struct {
	Candidates []config.Rule
}

func (e *AmbiguousError) Error() string {
	return fmt.Sprintf("proposal: delete: criteria match %d rules; candidates returned for disambiguation", len(e.Candidates))
}

// NoMatchError reports that a delete action's criteria match no rule.
type NoMatchError struct {
	Match config.RuleMatch
}

func (e *NoMatchError) Error() string {
	return "proposal: delete: no rule matches " + matchString(e.Match)
}

// New creates a validated, pending proposal. The id must be non-empty
// and unique to the caller; expiresAt must be after creation, so an
// unanswered proposal cannot linger forever.
func New(id string, action Action, requestedBy model.AgentIdentity, reason string, expiresAt time.Time) (*Proposal, error) {
	p := &Proposal{
		ID:          id,
		RequestedBy: requestedBy,
		Reason:      reason,
		Action:      action,
		State:       StatePending,
		CreatedAt:   time.Now(),
		ExpiresAt:   expiresAt,
	}
	if err := p.Validate(); err != nil {
		return nil, err
	}
	return p, nil
}

// Validate reports whether the proposal is well-formed enough to present
// to a human. New calls it, and it must be called again on any proposal
// received from storage or transport before rendering.
func (p *Proposal) Validate() error {
	if p.ID == "" {
		return errors.New("proposal: validate: empty id")
	}
	if p.Reason == "" {
		return errors.New("proposal: validate: empty reason")
	}
	switch p.State {
	case StatePending, StateApplied, StateRejected, StateExpired:
	default:
		return fmt.Errorf("proposal: validate: unknown state %q", p.State)
	}
	if (p.Action.Set == nil) == (p.Action.Delete == nil) {
		return errors.New("proposal: validate: action must be exactly one of set or delete")
	}
	if p.Action.Set != nil {
		if !validDecision(p.Action.Set.Decision) {
			return fmt.Errorf("proposal: validate: set: invalid decision %q", p.Action.Set.Decision)
		}
		if emptyMatch(p.Action.Set.Match) {
			return errors.New("proposal: validate: set: match must specify at least one criterion")
		}
	}
	if p.CreatedAt.IsZero() {
		return errors.New("proposal: validate: empty created_at")
	}
	if p.ExpiresAt.IsZero() {
		return errors.New("proposal: validate: empty expiry")
	}
	if !p.ExpiresAt.After(p.CreatedAt) {
		return errors.New("proposal: validate: expiry must be after creation")
	}
	return nil
}

// Expire transitions an unanswered pending proposal to expired when now
// is past its expiry. It reports whether the transition happened; a
// proposal that was already decided (applied or rejected) or already
// expired is left untouched.
func (p *Proposal) Expire(now time.Time) bool {
	if p.State != StatePending {
		return false
	}
	if !now.After(p.ExpiresAt) {
		return false
	}
	p.State = StateExpired
	return true
}

// Apply applies a pending proposal to the rule set and emits an audit
// record for the application. It requires an explicit human decision
// (non-empty appliedBy) and an audit sink; without either, nothing is
// applied. A proposal past its expiry is expired — never granted — and a
// proposal in any state other than pending cannot be applied.
//
// For a set action the returned rule set is the input with the rule
// upserted; for a delete action the uniquely matched rule is removed.
// The input slice is never mutated. The audit record identifies the
// resulting rule and the human who applied it.
func (p *Proposal) Apply(rules []config.Rule, appliedBy string, now time.Time, sink Sink) ([]config.Rule, error) {
	if err := p.Validate(); err != nil {
		return nil, fmt.Errorf("proposal: apply: %w", err)
	}
	if p.State != StatePending {
		return nil, fmt.Errorf("proposal: apply: cannot apply %s proposal %s", p.State, p.ID)
	}
	if appliedBy == "" {
		return nil, errors.New("proposal: apply: explicit human decision required (applied_by is empty)")
	}
	if sink == nil {
		return nil, errors.New("proposal: apply: audit sink required")
	}
	if now.After(p.ExpiresAt) {
		p.State = StateExpired
		return nil, fmt.Errorf("proposal: apply: proposal %s expired at %s and was not granted", p.ID, p.ExpiresAt.Format(time.RFC3339))
	}

	next := append([]config.Rule(nil), rules...)
	action := "set"
	var result config.Rule
	if p.Action.Set != nil {
		result = *p.Action.Set
		next = upsertRule(next, result)
	} else {
		action = "delete"
		matches, err := resolveDelete(p.Action.Delete.Match, rules)
		if err != nil {
			return nil, fmt.Errorf("proposal: apply: delete %s: %w", p.ID, err)
		}
		result = matches[0]
		next = removeRule(next, result)
	}

	ruleData, err := json.Marshal(result)
	if err != nil {
		return nil, fmt.Errorf("proposal: apply: marshal resulting rule: %w", err)
	}
	record := audit.ProposalApplied{
		ProposalID: p.ID,
		Action:     action,
		Rule:       string(ruleData),
		AppliedBy:  appliedBy,
		AppliedAt:  now.UTC().Format(time.RFC3339),
	}
	if err := sink.AppendProposalApplied(record); err != nil {
		return nil, fmt.Errorf("proposal: apply: audit: %w", err)
	}

	p.State = StateApplied
	p.AppliedAt = now
	return next, nil
}

// Reject marks a pending proposal rejected by a human. Like Apply it
// requires an explicit human decision (non-empty rejectedBy) and a
// reason; a proposal that was already decided cannot be rejected.
func (p *Proposal) Reject(rejectedBy, reason string, now time.Time) error {
	if p.State != StatePending {
		return fmt.Errorf("proposal: reject: cannot reject %s proposal %s", p.State, p.ID)
	}
	if rejectedBy == "" {
		return errors.New("proposal: reject: explicit human decision required (rejected_by is empty)")
	}
	if reason == "" {
		return errors.New("proposal: reject: empty reason")
	}
	p.State = StateRejected
	p.RejectedBy = rejectedBy
	p.RejectedReason = reason
	p.RejectedAt = now
	return nil
}

// resolveDelete finds the rules matching the delete criteria, applying
// the package-doc ambiguity rules: zero matches is a NoMatchError, one
// match resolves, and multiple matches return an AmbiguousError with the
// candidate list.
func resolveDelete(m config.RuleMatch, rules []config.Rule) ([]config.Rule, error) {
	var matches []config.Rule
	for _, r := range rules {
		if matchCriteria(m, r.Match) {
			matches = append(matches, r)
		}
	}
	switch len(matches) {
	case 0:
		return nil, &NoMatchError{Match: m}
	case 1:
		return matches, nil
	default:
		return nil, &AmbiguousError{Candidates: matches}
	}
}

// matchCriteria reports whether a rule's match satisfies the (possibly
// partial) criteria: every non-empty scalar criterion must equal the
// rule's field, and a non-empty CommandContains list must equal the
// rule's list element-wise.
func matchCriteria(c config.RuleMatch, m config.RuleMatch) bool {
	if c.Server != "" && c.Server != m.Server {
		return false
	}
	if c.Tool != "" && c.Tool != m.Tool {
		return false
	}
	if c.Capability != "" && c.Capability != m.Capability {
		return false
	}
	if len(c.CommandContains) > 0 && !equalStrings(c.CommandContains, m.CommandContains) {
		return false
	}
	return true
}

// matchesEqual reports whether two match criteria are identical,
// including empty CommandContains lists.
func matchesEqual(a, b config.RuleMatch) bool {
	return a.Server == b.Server && a.Tool == b.Tool && a.Capability == b.Capability &&
		equalStrings(a.CommandContains, b.CommandContains)
}

// upsertRule returns a new rule list with the rule upserted: a rule
// with an identical match is replaced in place (keeping its position),
// otherwise the rule is appended.
func upsertRule(rules []config.Rule, rule config.Rule) []config.Rule {
	next := append([]config.Rule(nil), rules...)
	for i, r := range next {
		if matchesEqual(r.Match, rule.Match) {
			next[i] = rule
			return next
		}
	}
	return append(next, rule)
}

// removeRule returns a new rule list without the uniquely matched rule.
func removeRule(rules []config.Rule, target config.Rule) []config.Rule {
	next := append([]config.Rule(nil), rules...)
	for i, r := range next {
		if matchesEqual(r.Match, target.Match) {
			return append(next[:i], next[i+1:]...)
		}
	}
	return next
}

// validDecision reports whether d is one of the six policy decisions
// accepted by the config schema (mirrors config.validate).
func validDecision(d config.Decision) bool {
	switch d {
	case config.Allow, config.Ask, config.Deny, config.Redact, config.ReadOnly, config.Sandbox:
		return true
	}
	return false
}

// emptyMatch reports whether a rule match specifies no criteria at all.
func emptyMatch(m config.RuleMatch) bool {
	return m.Server == "" && m.Tool == "" && m.Capability == "" && len(m.CommandContains) == 0
}

// equalStrings compares two string slices element-wise.
func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// matchString renders match criteria in compact form for error messages.
func matchString(m config.RuleMatch) string {
	var parts []string
	if m.Server != "" {
		parts = append(parts, "server="+m.Server)
	}
	if m.Tool != "" {
		parts = append(parts, "tool="+m.Tool)
	}
	if m.Capability != "" {
		parts = append(parts, "capability="+m.Capability)
	}
	if len(m.CommandContains) > 0 {
		parts = append(parts, "command_contains="+strings.Join(m.CommandContains, ","))
	}
	if len(parts) == 0 {
		return "{}"
	}
	return "{" + strings.Join(parts, " ") + "}"
}
