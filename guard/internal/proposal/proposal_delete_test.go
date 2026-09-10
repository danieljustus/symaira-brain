package proposal

import (
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/config"
)

// Criterion (f): delete-by-host ambiguity, unique match applies.
func TestApply_Delete_UniqueMatch(t *testing.T) {
	rules := []config.Rule{
		testRule("symseek", config.Allow),
		testRule("filesystem", config.Allow),
	}
	p := newTestProposal(t, "prop-1", deleteAction(config.RuleMatch{Server: "symseek"}))
	sink := &fakeSink{}
	got, err := p.Apply(rules, "human", time.Now(), sink)
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(got) != 1 || got[0].Match.Server != "filesystem" {
		t.Errorf("rules after delete = %+v, want only filesystem", got)
	}
	if p.State != StateApplied {
		t.Errorf("State = %s, want applied", p.State)
	}
	if len(sink.records) != 1 || sink.records[0].Action != "delete" {
		t.Fatalf("sink records = %+v, want one delete record", sink.records)
	}
	var removed config.Rule
	if err := json.Unmarshal([]byte(sink.records[0].Rule), &removed); err != nil {
		t.Fatalf("record rule not JSON: %v", err)
	}
	if removed.Match.Server != "symseek" || removed.Decision != config.Allow {
		t.Errorf("record rule = %+v, want removed symseek/allow rule", removed)
	}
}

// Criterion (f): multiple matches return the candidate list.
func TestApply_Delete_AmbiguousReturnsCandidates(t *testing.T) {
	rules := []config.Rule{
		testRule("symseek", config.Allow),
		testRule("symseek", config.Ask),
		testRule("filesystem", config.Allow),
	}
	p := newTestProposal(t, "prop-1", deleteAction(config.RuleMatch{Server: "symseek"}))
	sink := &fakeSink{}
	got, err := p.Apply(rules, "human", time.Now(), sink)
	var amb *AmbiguousError
	if !errors.As(err, &amb) {
		t.Fatalf("Apply() error = %v, want *AmbiguousError", err)
	}
	if len(amb.Candidates) != 2 {
		t.Errorf("Candidates = %d, want 2", len(amb.Candidates))
	}
	if got != nil {
		t.Errorf("Apply() returned rules %+v on ambiguity, want nil", got)
	}
	if p.State != StatePending {
		t.Errorf("State = %s, want pending", p.State)
	}
	if len(sink.records) != 0 {
		t.Error("ambiguous apply emitted audit record, want none")
	}
}

// Criterion (f): no match is an error.
func TestApply_Delete_NoMatch(t *testing.T) {
	rules := []config.Rule{testRule("filesystem", config.Allow)}
	p := newTestProposal(t, "prop-1", deleteAction(config.RuleMatch{Server: "symseek"}))
	sink := &fakeSink{}
	got, err := p.Apply(rules, "human", time.Now(), sink)
	var noMatch *NoMatchError
	if !errors.As(err, &noMatch) {
		t.Fatalf("Apply() error = %v, want *NoMatchError", err)
	}
	if got != nil {
		t.Errorf("Apply() returned rules %+v on no match, want nil", got)
	}
	if p.State != StatePending {
		t.Errorf("State = %s, want pending", p.State)
	}
	if len(sink.records) != 0 {
		t.Error("no-match apply emitted audit record, want none")
	}
}

// A delete that omits its identity entirely matches every rule: it can
// only ever apply when the rule set is a singleton — otherwise it is
// ambiguous or a no-match — and the audit record identifies the rule it
// removed.
func TestApply_Delete_WithoutIdentity(t *testing.T) {
	// One rule: a unique match, so the delete applies and the audit
	// record names the removed rule.
	one := []config.Rule{testRule("symseek", config.Allow)}
	p := newTestProposal(t, "prop-1", deleteAction(config.RuleMatch{}))
	sink := &fakeSink{}
	got, err := p.Apply(one, "human", time.Now(), sink)
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(got) != 0 {
		t.Errorf("rules after identity-less delete = %+v, want none", got)
	}
	if p.State != StateApplied {
		t.Errorf("State = %s, want applied", p.State)
	}
	if len(sink.records) != 1 {
		t.Fatalf("sink records = %d, want 1", len(sink.records))
	}
	var removed config.Rule
	if err := json.Unmarshal([]byte(sink.records[0].Rule), &removed); err != nil {
		t.Fatalf("record rule not JSON: %v", err)
	}
	if removed.Match.Server != "symseek" {
		t.Errorf("record rule = %+v, want removed symseek rule", removed)
	}

	// Two rules: ambiguous, nothing removed, candidates returned.
	two := []config.Rule{testRule("symseek", config.Allow), testRule("filesystem", config.Allow)}
	p2 := newTestProposal(t, "prop-2", deleteAction(config.RuleMatch{}))
	got2, err := p2.Apply(two, "human", time.Now(), &fakeSink{})
	var amb *AmbiguousError
	if !errors.As(err, &amb) {
		t.Fatalf("Apply() identity-less delete error = %v, want *AmbiguousError", err)
	}
	if len(amb.Candidates) != 2 {
		t.Errorf("Candidates = %d, want 2", len(amb.Candidates))
	}
	if got2 != nil || p2.State != StatePending {
		t.Errorf("identity-less delete applied: got %+v, state %s", got2, p2.State)
	}

	// No rules: no match.
	p3 := newTestProposal(t, "prop-3", deleteAction(config.RuleMatch{}))
	if _, err := p3.Apply(nil, "human", time.Now(), &fakeSink{}); err == nil {
		t.Fatal("Apply() identity-less delete on empty rules = nil error, want error")
	}
}

func TestReject(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	now := time.Now()
	if err := p.Reject("human", "not now", now); err != nil {
		t.Fatalf("Reject: %v", err)
	}
	if p.State != StateRejected {
		t.Errorf("State = %s, want rejected", p.State)
	}
	if p.RejectedBy != "human" || p.RejectedReason != "not now" || !p.RejectedAt.Equal(now) {
		t.Errorf("rejection provenance = %+v, want human/not now/%v", p, now)
	}

	// A decided proposal cannot be rejected again.
	if err := p.Reject("human", "again", now); err == nil {
		t.Error("Reject() on rejected proposal = nil error, want error")
	}

	// Rejection is also a human decision.
	q := newTestProposal(t, "prop-2", setAction(testRule("symseek", config.Allow)))
	if err := q.Reject("", "why", now); err == nil {
		t.Error("Reject() with empty rejected_by = nil error, want error")
	}

	// A rejection without a reason is not a decision.
	r := newTestProposal(t, "prop-3", setAction(testRule("symseek", config.Allow)))
	if err := r.Reject("human", "", now); err == nil {
		t.Error("Reject() with empty reason = nil error, want error")
	}
}
