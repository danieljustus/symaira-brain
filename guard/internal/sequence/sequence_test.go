package sequence

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// attempt builds a proxy-sourced ActionEvent in the requested state.
func attempt(tool string, args any) model.ActionEvent {
	return seqEvent(tool, args, model.ActionRequested)
}

// seqEvent builds an ActionEvent with the given tool call and state.
func seqEvent(tool string, args any, state model.ActionState) model.ActionEvent {
	return model.ActionEvent{
		ID:        "evt_1_proxy_test",
		SchemaVer: model.SchemaVersion,
		Source:    model.SourceProxy,
		Agent:     model.AgentIdentity{AgentID: "test-agent"},
		Call:      model.ToolCall{Server: "test-server", Tool: tool, Args: args},
		State:     state,
		Timestamp: time.Now().UTC().Format(time.RFC3339),
	}
}

// runEvents feeds each event to the detector and returns the decisions.
func runEvents(d *Detector, events []model.ActionEvent) []model.Decision {
	got := make([]model.Decision, 0, len(events))
	for _, ev := range events {
		got = append(got, d.Evaluate(ev).Decision)
	}
	return got
}

func equalDecisions(a, b []model.Decision) bool {
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

func TestEvaluate_ExactRepetition(t *testing.T) {
	read := func(path string) model.ActionEvent {
		return attempt("read_file", map[string]any{"path": path})
	}
	tests := []struct {
		name   string
		cfg    Config
		events []model.ActionEvent
		want   []model.Decision
	}{
		{
			name:   "blocks third identical call at default threshold",
			cfg:    Config{Enabled: true},
			events: []model.ActionEvent{read("a"), read("a"), read("a")},
			want:   []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionDeny},
		},
		{
			name: "stays blocked while repetition is in window",
			cfg:  Config{Enabled: true},
			events: []model.ActionEvent{
				read("a"), read("a"), read("a"), read("a"), read("a"),
			},
			want: []model.Decision{
				model.DecisionAllow, model.DecisionAllow,
				model.DecisionDeny, model.DecisionDeny, model.DecisionDeny,
			},
		},
		{
			name: "different input on same tool has its own key",
			cfg:  Config{Enabled: true},
			events: []model.ActionEvent{
				read("a"), read("a"), read("b"), read("a"),
			},
			want: []model.Decision{
				model.DecisionAllow, model.DecisionAllow,
				model.DecisionAllow, model.DecisionDeny,
			},
		},
		{
			name: "different tool with same args not blocked",
			cfg:  Config{Enabled: true},
			events: []model.ActionEvent{
				attempt("read_file", "x"), attempt("write_file", "x"), attempt("read_file", "x"),
			},
			want: []model.Decision{
				model.DecisionAllow, model.DecisionAllow, model.DecisionAllow,
			},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			d := NewDetector(tt.cfg)
			got := runEvents(d, tt.events)
			if !equalDecisions(got, tt.want) {
				t.Errorf("decisions = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestEvaluate_ConfigurableThreshold(t *testing.T) {
	read := func(p string) model.ActionEvent { return attempt("read_file", p) }
	tests := []struct {
		name   string
		cfg    Config
		events []model.ActionEvent
		want   []model.Decision
	}{
		{
			name:   "threshold two blocks second call",
			cfg:    Config{Enabled: true, Threshold: 2},
			events: []model.ActionEvent{read("a"), read("a"), read("a")},
			want:   []model.Decision{model.DecisionAllow, model.DecisionDeny, model.DecisionDeny},
		},
		{
			name:   "threshold five blocks fifth call",
			cfg:    Config{Enabled: true, Threshold: 5},
			events: []model.ActionEvent{read("a"), read("a"), read("a"), read("a"), read("a"), read("a")},
			want: []model.Decision{
				model.DecisionAllow, model.DecisionAllow, model.DecisionAllow,
				model.DecisionAllow, model.DecisionDeny, model.DecisionDeny,
			},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			d := NewDetector(tt.cfg)
			got := runEvents(d, tt.events)
			if !equalDecisions(got, tt.want) {
				t.Errorf("decisions = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestEvaluate_FuzzySearchDedup(t *testing.T) {
	search := func(q string) model.ActionEvent { return attempt("search_web", q) }
	find := func(q string) model.ActionEvent { return attempt("find_files", q) }
	run := func(q string) model.ActionEvent { return attempt("run_command", q) }
	tests := []struct {
		name   string
		events []model.ActionEvent
		want   []model.Decision
	}{
		{
			name: "near-identical search queries count as one repetition",
			events: []model.ActionEvent{
				search("find notes about project alpha"),
				search("find notes about project alpha please"),
				search("find notes about project alpha"),
			},
			want: []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionDeny},
		},
		{
			name: "distinct search query has its own key",
			events: []model.ActionEvent{
				search("find notes about project alpha"),
				search("find notes about project alpha"),
				search("search web for weather in berlin"),
			},
			want: []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionAllow},
		},
		{
			name: "find-prefixed tool is search-shaped",
			events: []model.ActionEvent{
				find("please locate the todo list"),
				find("please locate the todo list now"),
				find("please locate the todo list"),
			},
			want: []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionDeny},
		},
		{
			name: "non-search tool with similar input is not fuzzy blocked",
			events: []model.ActionEvent{
				run("echo hello"),
				run("echo hello world"),
				run("echo hello world again"),
			},
			want: []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionAllow},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			d := NewDetector(Config{Enabled: true})
			got := runEvents(d, tt.events)
			if !equalDecisions(got, tt.want) {
				t.Errorf("decisions = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestEvaluate_WindowEviction(t *testing.T) {
	read := func(p string) model.ActionEvent { return attempt("read_file", p) }
	d := NewDetector(Config{Enabled: true, WindowSize: 2})

	events := []model.ActionEvent{
		read("a"), read("b"), read("c"), // c evicts the least recently used signature (a)
		read("a"), read("a"), read("a"), // a is re-learned and blocks again at threshold 3
	}
	want := []model.Decision{
		model.DecisionAllow, model.DecisionAllow, model.DecisionAllow,
		model.DecisionAllow, model.DecisionAllow, model.DecisionDeny,
	}
	got := runEvents(d, events)
	if !equalDecisions(got, want) {
		t.Errorf("decisions = %v, want %v", got, want)
	}
	if len(d.entries) > 2 {
		t.Errorf("window holds %d entries, want at most %d", len(d.entries), 2)
	}
}

func TestEvaluate_Disabled(t *testing.T) {
	d := NewDetector(Config{}) // Enabled defaults to false
	events := []model.ActionEvent{attempt("read_file", "a"), attempt("read_file", "a")}
	want := []model.Decision{model.DecisionAllow, model.DecisionAllow}
	if got := runEvents(d, events); !equalDecisions(got, want) {
		t.Errorf("decisions = %v, want %v", got, want)
	}
	got := d.Evaluate(attempt("read_file", "a"))
	if got.Decision != model.DecisionAllow || got.Reason != ReasonPrefix+"disabled" {
		t.Errorf("disabled evaluation = %+v, want allow with %q reason", got, ReasonPrefix+"disabled")
	}
	if sum := d.Ledger().Summarize(); len(sum) != 0 {
		t.Errorf("disabled detector recorded ledger entries: %v", sum)
	}
}

func TestEvaluate_NonAttemptStates(t *testing.T) {
	d := NewDetector(Config{Enabled: true})
	events := []model.ActionEvent{
		seqEvent("read_file", "a", model.ActionRequested),
		seqEvent("read_file", "a", model.ActionApproved),
		seqEvent("read_file", "a", model.ActionStarted),
		seqEvent("read_file", "a", model.ActionCompleted),
		seqEvent("read_file", "a", model.ActionRequested),
	}
	want := []model.Decision{
		model.DecisionAllow, model.DecisionAllow, model.DecisionAllow,
		model.DecisionAllow, model.DecisionDeny,
	}
	got := runEvents(d, events)
	if !equalDecisions(got, want) {
		t.Errorf("decisions = %v, want %v", got, want)
	}
}

func TestEvaluate_ArgsCanonicalization(t *testing.T) {
	d := NewDetector(Config{Enabled: true})
	events := []model.ActionEvent{
		attempt("read_file", map[string]any{"path": "/tmp/x", "recursive": true}),
		attempt("read_file", map[string]any{"recursive": true, "path": "/tmp/x"}),
		attempt("read_file", map[string]any{"path": "/tmp/x", "recursive": true}),
	}
	want := []model.Decision{model.DecisionAllow, model.DecisionAllow, model.DecisionDeny}
	got := runEvents(d, events)
	if !equalDecisions(got, want) {
		t.Errorf("decisions = %v, want %v", got, want)
	}
}

func TestEvaluate_DenyContract(t *testing.T) {
	d := NewDetector(Config{Enabled: true})
	ev := attempt("read_file", "a")
	d.Evaluate(ev)
	d.Evaluate(ev)
	got := d.Evaluate(ev)

	if got.Decision != model.DecisionDeny {
		t.Fatalf("decision = %q, want deny", got.Decision)
	}
	if !got.Recoverable {
		t.Error("deny must be recoverable, got Recoverable=false")
	}
	if !strings.HasPrefix(got.Reason, ReasonPrefix) {
		t.Errorf("reason %q must start with %q", got.Reason, ReasonPrefix)
	}
	if got.Key == "" {
		t.Error("deny must carry the matched window key")
	}
	if got.Count != 3 {
		t.Errorf("Count = %d, want 3", got.Count)
	}

	// A deny must not poison unrelated calls: a different call goes through.
	other := d.Evaluate(attempt("read_file", "b"))
	if other.Decision != model.DecisionAllow {
		t.Errorf("unrelated call decision = %q, want allow", other.Decision)
	}
	if other.Recoverable {
		t.Error("allow must not be marked recoverable")
	}
}
