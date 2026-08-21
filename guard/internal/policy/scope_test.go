package policy

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

func TestScopeCeiling(t *testing.T) {
	allow := Result{Decision: model.DecisionAllow, Matched: true, Reason: "rule allows shell"}
	deny := Result{Decision: model.DecisionDeny, Matched: true, Reason: "rule denies network"}
	ask := Result{Decision: model.DecisionAsk, Matched: false, Reason: "default ask"}

	tests := []struct {
		name       string
		scope      []string
		capability string
		result     Result
		want       model.Decision
		wantReason string // non-empty: narrowed reason must contain this
	}{
		{"in-scope allow passes through", []string{"shell"}, "shell", allow, model.DecisionAllow, ""},
		{"wildcard scope passes through", []string{"*"}, "read_secret", allow, model.DecisionAllow, ""},
		{"narrower scope set still passes", []string{"read_public", "read_private"}, "read_private", allow, model.DecisionAllow, ""},
		{"out-of-scope allow narrowed to deny", []string{"read_public"}, "shell", allow, model.DecisionDeny, "not granted by token scope"},
		{"out-of-scope ask narrowed to deny", []string{"read_public"}, "network", ask, model.DecisionDeny, "not granted by token scope"},
		{"already denied passes through", []string{"*"}, "network", deny, model.DecisionDeny, "rule denies network"},
		{"empty scope denies everything", nil, "read_public", allow, model.DecisionDeny, "not granted by token scope"},
		{"empty capability never in scope", []string{"*"}, "", allow, model.DecisionDeny, "not granted by token scope"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ScopeCeiling(tt.scope, tt.capability, tt.result)
			if got.Decision != tt.want {
				t.Errorf("Decision = %q, want %q", got.Decision, tt.want)
			}
			if tt.wantReason != "" && !strings.Contains(got.Reason, tt.wantReason) {
				t.Errorf("Reason = %q, want it to contain %q", got.Reason, tt.wantReason)
			}
		})
	}
}

func TestScopeCeiling_NarrowedResultShape(t *testing.T) {
	got := ScopeCeiling([]string{"read_public"}, "shell", Result{Decision: model.DecisionAllow, Matched: true})
	if got.Matched {
		t.Error("narrowed result must not be a rule match")
	}
	if got.Rule != nil {
		t.Errorf("narrowed result Rule = %v, want nil", got.Rule)
	}
}
