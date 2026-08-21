package policy

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
	"github.com/danieljustus/symaira-guard/internal/sequence"
)

func TestSequenceRule_Evaluate(t *testing.T) {
	r := NewSequenceRule(sequence.Config{Enabled: true})
	ev := func(args string) model.ActionEvent {
		return model.ActionEvent{
			SchemaVer: model.SchemaVersion,
			Source:    model.SourceProxy,
			Agent:     model.AgentIdentity{AgentID: "test-agent"},
			Call:      model.ToolCall{Server: "s", Tool: "read_file", Args: args},
			State:     model.ActionRequested,
		}
	}

	first := r.Evaluate(ev("a"))
	if first.Decision != model.DecisionAllow || first.Matched {
		t.Errorf("first call: got %+v, want allow without match", first)
	}
	if second := r.Evaluate(ev("a")); second.Decision != model.DecisionAllow {
		t.Errorf("second call: got %+v, want allow", second)
	}

	got := r.Evaluate(ev("a"))
	if got.Decision != model.DecisionDeny {
		t.Fatalf("third call: decision = %q, want deny", got.Decision)
	}
	if !got.Matched {
		t.Error("third call: Matched = false, want true")
	}
	if !strings.HasPrefix(got.Reason, sequence.ReasonPrefix) {
		t.Errorf("reason %q must carry the sequence prefix", got.Reason)
	}
}

func TestSequenceRule_DisabledAlwaysAllows(t *testing.T) {
	r := NewSequenceRule(sequence.Config{}) // Enabled defaults to false
	ev := model.ActionEvent{
		SchemaVer: model.SchemaVersion,
		Source:    model.SourceProxy,
		Agent:     model.AgentIdentity{AgentID: "test-agent"},
		Call:      model.ToolCall{Server: "s", Tool: "read_file", Args: "a"},
		State:     model.ActionRequested,
	}
	for i := 0; i < 5; i++ {
		if got := r.Evaluate(ev); got.Decision != model.DecisionAllow {
			t.Fatalf("call %d: decision = %q, want allow", i+1, got.Decision)
		}
	}
}
