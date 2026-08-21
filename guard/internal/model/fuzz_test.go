package model

import (
	"strings"
	"testing"
)

// FuzzValidateSource tests that arbitrary source type strings don't panic.
func FuzzValidateSource(f *testing.F) {
	seeds := []string{"proxy", "hook", "artifact", "scan", "unknown", "", "\x00", strings.Repeat("A", 1000)}
	for _, s := range seeds {
		f.Add(s)
	}

	f.Fuzz(func(t *testing.T, s string) {
		// Should never panic, only return an error for invalid types.
		_ = ValidateSource(SourceType(s))
	})
}

// FuzzValidateDecision tests that arbitrary decision strings don't panic.
func FuzzValidateDecision(f *testing.F) {
	seeds := []string{"allow", "deny", "ask", "nope", "", "\n", "\x00"}
	for _, s := range seeds {
		f.Add(s)
	}

	f.Fuzz(func(t *testing.T, s string) {
		_ = ValidateDecision(Decision(s))
	})
}

// FuzzValidateState tests that arbitrary state strings don't panic.
func FuzzValidateState(f *testing.F) {
	seeds := []string{"requested", "completed", "", "unknown"}
	for _, s := range seeds {
		f.Add(s)
	}

	f.Fuzz(func(t *testing.T, s string) {
		_ = ValidateState(ActionState(s))
	})
}

// FuzzEventID tests that event ID generation handles all source types safely.
func FuzzEventID(f *testing.F) {
	seeds := []string{"proxy", "hook", "", "\x00", "proxy\x00"}
	for _, s := range seeds {
		f.Add(s, int64(0))
		f.Add(s, int64(-1))
		f.Add(s, int64(1<<62))
	}

	f.Fuzz(func(t *testing.T, source string, counter int64) {
		_ = EventID(SourceType(source), counter)
	})
}
