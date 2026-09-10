package proposal

import (
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/audit"
	"github.com/danieljustus/symaira-brain/guard/internal/config"
	"github.com/danieljustus/symaira-brain/guard/internal/model"
)

func testAgent() model.AgentIdentity {
	return model.AgentIdentity{AgentID: "agent-a", SessionID: "sess-1"}
}

func testRule(server string, decision config.Decision) config.Rule {
	return config.Rule{
		Match:    config.RuleMatch{Server: server},
		Decision: decision,
	}
}

func setAction(rule config.Rule) Action {
	return Action{Set: &rule}
}

func deleteAction(match config.RuleMatch) Action {
	return Action{Delete: &Delete{Match: match}}
}

func newTestProposal(t *testing.T, id string, action Action) *Proposal {
	t.Helper()
	p, err := New(id, action, testAgent(), "grant access for this job", time.Now().Add(time.Hour))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return p
}

// fakeSink captures emitted audit records; set err to make appends fail.
type fakeSink struct {
	records []audit.ProposalApplied
	err     error
}

func (f *fakeSink) AppendProposalApplied(r audit.ProposalApplied) error {
	if f.err != nil {
		return f.err
	}
	f.records = append(f.records, r)
	return nil
}

func TestNew(t *testing.T) {
	tests := []struct {
		name      string
		id        string
		action    Action
		reason    string
		expiresIn time.Duration
		wantErr   bool
	}{
		{"valid set", "prop-1", setAction(testRule("symseek", config.Allow)), "needed for job", time.Hour, false},
		{"valid delete", "prop-2", deleteAction(config.RuleMatch{Server: "symseek"}), "no longer needed", time.Hour, false},
		{"empty id", "", setAction(testRule("symseek", config.Allow)), "needed", time.Hour, true},
		{"both set and delete", "prop-3", Action{Set: &config.Rule{Match: config.RuleMatch{Server: "s"}, Decision: config.Allow}, Delete: &Delete{}}, "confused", time.Hour, true},
		{"neither set nor delete", "prop-4", Action{}, "nothing", time.Hour, true},
		{"invalid decision", "prop-5", setAction(testRule("symseek", config.Decision("maybe"))), "needed", time.Hour, true},
		{"set without match", "prop-6", setAction(config.Rule{Decision: config.Allow}), "needed", time.Hour, true},
		{"empty reason", "prop-7", setAction(testRule("symseek", config.Allow)), "", time.Hour, true},
		{"expiry in past", "prop-8", setAction(testRule("symseek", config.Allow)), "needed", -time.Hour, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := New(tt.id, tt.action, testAgent(), tt.reason, time.Now().Add(tt.expiresIn))
			if (err != nil) != tt.wantErr {
				t.Errorf("New() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestProposal_Validate(t *testing.T) {
	valid := func() *Proposal {
		return &Proposal{
			ID:          "prop-1",
			RequestedBy: testAgent(),
			Reason:      "needed",
			Action:      setAction(testRule("symseek", config.Allow)),
			State:       StatePending,
			CreatedAt:   time.Now(),
			ExpiresAt:   time.Now().Add(time.Hour),
		}
	}
	tests := []struct {
		name    string
		mutate  func(*Proposal)
		wantErr bool
	}{
		{"valid", func(*Proposal) {}, false},
		{"empty id", func(p *Proposal) { p.ID = "" }, true},
		{"empty reason", func(p *Proposal) { p.Reason = "" }, true},
		{"unknown state", func(p *Proposal) { p.State = State("limbo") }, true},
		{"zero created_at", func(p *Proposal) { p.CreatedAt = time.Time{} }, true},
		{"zero expiry", func(p *Proposal) { p.ExpiresAt = time.Time{} }, true},
		{"expiry before creation", func(p *Proposal) { p.ExpiresAt = p.CreatedAt.Add(-time.Hour) }, true},
		{"delete without identity is valid", func(p *Proposal) { p.Action = deleteAction(config.RuleMatch{}) }, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := valid()
			tt.mutate(p)
			err := p.Validate()
			if (err != nil) != tt.wantErr {
				t.Errorf("Validate() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestProposal_JSONRoundTrip(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	data, err := json.Marshal(p)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	var got Proposal
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if err := got.Validate(); err != nil {
		t.Fatalf("round-tripped proposal invalid: %v", err)
	}
	if got.ID != p.ID || got.State != StatePending || got.Action.Set == nil ||
		got.Action.Set.Decision != config.Allow || got.Action.Set.Match.Server != "symseek" {
		t.Errorf("round trip mismatch: %+v", got)
	}
}

func TestProposal_Expire(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	before := p.ExpiresAt.Add(-time.Hour)
	if p.Expire(before) {
		t.Error("Expire() = true before expiry, want false")
	}
	if p.State != StatePending {
		t.Errorf("State = %s after early Expire, want pending", p.State)
	}
	after := p.ExpiresAt.Add(time.Minute)
	if !p.Expire(after) {
		t.Error("Expire() = false past expiry, want true")
	}
	if p.State != StateExpired {
		t.Errorf("State = %s after Expire, want expired", p.State)
	}

	rejected := newTestProposal(t, "prop-2", setAction(testRule("symseek", config.Allow)))
	if err := rejected.Reject("human", "no", time.Now()); err != nil {
		t.Fatalf("Reject: %v", err)
	}
	if rejected.Expire(after) {
		t.Error("Expire() = true on rejected proposal, want false")
	}
}

func TestApply_Set_UpsertsRule(t *testing.T) {
	existing := []config.Rule{
		testRule("symseek", config.Deny),
		testRule("filesystem", config.Allow),
	}

	// Same match, new decision: replaced in place, position kept.
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Ask)))
	got, err := p.Apply(existing, "human", time.Now(), &fakeSink{})
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(got) != 2 || got[0].Decision != config.Ask || got[1].Match.Server != "filesystem" {
		t.Errorf("rules after upsert = %+v, want symseek=ask then filesystem", got)
	}
	if p.State != StateApplied {
		t.Errorf("State = %s, want applied", p.State)
	}

	// New match: appended.
	p2 := newTestProposal(t, "prop-2", setAction(testRule("network", config.Deny)))
	got2, err := p2.Apply(existing, "human", time.Now(), &fakeSink{})
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(got2) != 3 || got2[2].Match.Server != "network" {
		t.Errorf("rules after append = %+v, want network appended", got2)
	}

	// Idempotent: identical match and decision change nothing.
	p3 := newTestProposal(t, "prop-3", setAction(testRule("symseek", config.Deny)))
	got3, err := p3.Apply(existing, "human", time.Now(), &fakeSink{})
	if err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(got3) != 2 || got3[0].Decision != config.Deny {
		t.Errorf("rules after idempotent apply = %+v, want unchanged", got3)
	}

	// The caller's slice is never mutated.
	if len(existing) != 2 || existing[0].Decision != config.Deny {
		t.Errorf("input rules mutated: %+v", existing)
	}
}

func TestApply_EmitsAuditRecord(t *testing.T) {
	sink := &fakeSink{}
	p := newTestProposal(t, "prop-9", setAction(testRule("symseek", config.Allow)))
	now := time.Now()
	if _, err := p.Apply(nil, "human-1", now, sink); err != nil {
		t.Fatalf("Apply: %v", err)
	}
	if len(sink.records) != 1 {
		t.Fatalf("sink records = %d, want 1", len(sink.records))
	}
	rec := sink.records[0]
	if rec.ProposalID != "prop-9" {
		t.Errorf("ProposalID = %q, want prop-9", rec.ProposalID)
	}
	if rec.Action != "set" {
		t.Errorf("Action = %q, want set", rec.Action)
	}
	if rec.AppliedBy != "human-1" {
		t.Errorf("AppliedBy = %q, want human-1", rec.AppliedBy)
	}
	if rec.AppliedAt != now.UTC().Format(time.RFC3339) {
		t.Errorf("AppliedAt = %q, want %q", rec.AppliedAt, now.UTC().Format(time.RFC3339))
	}
	var rule config.Rule
	if err := json.Unmarshal([]byte(rec.Rule), &rule); err != nil {
		t.Fatalf("record rule not JSON: %v", err)
	}
	if rule.Match.Server != "symseek" || rule.Decision != config.Allow {
		t.Errorf("record rule = %+v, want symseek/allow", rule)
	}
}

// Criterion (c): an expired proposal is never applied — expiry does not grant.
func TestApply_ExpiredDoesNotGrant(t *testing.T) {
	// A proposal created two hours ago with a one-hour expiry: it has
	// sat unanswered past its expiry.
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	p.CreatedAt = time.Now().Add(-2 * time.Hour)
	p.ExpiresAt = p.CreatedAt.Add(time.Hour)

	rules := []config.Rule{testRule("symseek", config.Deny)}
	sink := &fakeSink{}
	got, err := p.Apply(rules, "human", time.Now(), sink)
	if err == nil {
		t.Fatal("Apply() past expiry = nil error, want error (expiry must not grant)")
	}
	if p.State != StateExpired {
		t.Errorf("State = %s, want expired", p.State)
	}
	if got != nil {
		t.Errorf("Apply() past expiry returned rules %+v, want nil", got)
	}
	if len(sink.records) != 0 {
		t.Error("expired apply emitted audit record, want none")
	}
	if len(rules) != 1 || rules[0].Decision != config.Deny {
		t.Errorf("input rules changed: %+v", rules)
	}
}

// Criterion (d): a proposal cannot be applied without an explicit human decision.
func TestApply_RequiresExplicitHumanDecision(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	sink := &fakeSink{}
	got, err := p.Apply(nil, "", time.Now(), sink)
	if err == nil {
		t.Fatal("Apply() with empty applied_by = nil error, want error")
	}
	if p.State != StatePending {
		t.Errorf("State = %s, want pending", p.State)
	}
	if got != nil {
		t.Errorf("Apply() returned rules %+v on refused apply, want nil", got)
	}
	if len(sink.records) != 0 {
		t.Error("refused apply emitted audit record, want none")
	}
}

func TestApply_RequiresAuditSink(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	if _, err := p.Apply(nil, "human", time.Now(), nil); err == nil {
		t.Fatal("Apply() with nil sink = nil error, want error")
	}
	if p.State != StatePending {
		t.Errorf("State = %s, want pending", p.State)
	}
}

func TestApply_NotPending(t *testing.T) {
	tests := []struct {
		name string
		to   State
	}{
		{"applied", StateApplied},
		{"rejected", StateRejected},
		{"expired", StateExpired},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
			p.State = tt.to
			if _, err := p.Apply(nil, "human", time.Now(), &fakeSink{}); err == nil {
				t.Errorf("Apply() on %s proposal = nil error, want error", tt.to)
			}
		})
	}
}

func TestApply_RejectsMalformedProposal(t *testing.T) {
	rule := testRule("symseek", config.Allow)
	p := &Proposal{
		ID:          "prop-1",
		RequestedBy: testAgent(),
		Reason:      "needed",
		Action:      Action{Set: &rule, Delete: &Delete{}},
		State:       StatePending,
		CreatedAt:   time.Now(),
		ExpiresAt:   time.Now().Add(time.Hour),
	}
	if _, err := p.Apply(nil, "human", time.Now(), &fakeSink{}); err == nil {
		t.Fatal("Apply() on malformed proposal = nil error, want error")
	}
	if p.State != StatePending {
		t.Errorf("State = %s, want pending", p.State)
	}
}

func TestApply_SinkErrorDoesNotApply(t *testing.T) {
	p := newTestProposal(t, "prop-1", setAction(testRule("symseek", config.Allow)))
	sink := &fakeSink{err: errors.New("disk full")}
	got, err := p.Apply(nil, "human", time.Now(), sink)
	if err == nil {
		t.Fatal("Apply() with failing sink = nil error, want error")
	}
	if p.State != StatePending {
		t.Errorf("State = %s after failed audit append, want pending", p.State)
	}
	if got != nil {
		t.Errorf("Apply() returned rules %+v on failed audit append, want nil", got)
	}
}
