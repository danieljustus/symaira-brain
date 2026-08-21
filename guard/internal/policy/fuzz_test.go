package policy

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// FuzzEvaluate tests that arbitrary tool calls don't panic during evaluation.
func FuzzEvaluate(f *testing.F) {
	c, err := NewCatalog([]Rule{
		{ID: "r1", Version: "1.0", Precedence: 10, Decision: model.DecisionDeny,
			Match: MatchCriteria{CommandContains: []string{"rm", "curl"}}},
		{ID: "r2", Version: "1.0", Precedence: 20, Decision: model.DecisionAllow,
			Match: MatchCriteria{Server: "test"}},
	}, "1.0.0")
	if err != nil {
		f.Fatalf("NewCatalog: %v", err)
	}

	seeds := []string{
		"", "test", "server", "tool", "shell",
		"\x00", strings.Repeat("A", 1000),
	}
	for _, s := range seeds {
		f.Add(s, s, s)
	}

	f.Fuzz(func(t *testing.T, server, tool, cap string) {
		call := model.ToolCall{
			Server:     server,
			Tool:       tool,
			Capability: cap,
			Args:       server, // use string args for CommandContains matching
		}
		_ = c.Evaluate(call, model.DecisionAllow)
		_ = c.EvaluateOpts(call, model.DecisionAllow, Options{Trace: true})
	})
}
