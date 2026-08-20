package extractor

import (
	"testing"

	"github.com/danieljustus/symaira-corekit/evidencekit"
)

func TestPatternExtraction(t *testing.T) {
	pe := NewPatternExtractor()

	tests := []struct {
		input            string
		expectedFact     string
		expectedCategory string
	}{
		{
			input:            "I like TypeScript.",
			expectedFact:     "User prefers TypeScript",
			expectedCategory: "preference",
		},
		{
			input:            "Ich mag golang.",
			expectedFact:     "User prefers golang",
			expectedCategory: "preference",
		},
		{
			input:            "I'm working on the symaira-memory project.",
			expectedFact:     "User is building the symaira-memory project",
			expectedCategory: "project",
		},
		{
			input:            "Ich bin ein Senior Entwickler.",
			expectedFact:     "User is a Senior Entwickler",
			expectedCategory: "identity",
		},
	}

	for _, tt := range tests {
		facts := pe.ExtractFacts(tt.input)
		if len(facts) == 0 {
			t.Errorf("input '%s' failed to extract any facts", tt.input)
			continue
		}

		matched := false
		for _, f := range facts {
			if f.Content == tt.expectedFact && f.Category == tt.expectedCategory {
				matched = true
				break
			}
		}

		if !matched {
			t.Errorf("expected fact '%s' (%s), got %+v", tt.expectedFact, tt.expectedCategory, facts)
		}
	}
}

func TestKeywordFilterFallback(t *testing.T) {
	pe := NewPatternExtractor()

	// Sentence with no explicit regex trigger, but high-value keywords
	input := "Wir planen das symaira projekt aufzubauen."
	facts := pe.ExtractFacts(input)

	if len(facts) == 0 {
		t.Fatalf("expected keyword fallback fact to be extracted")
	}

	if facts[0].Category != "general" {
		t.Errorf("expected category 'general', got '%s'", facts[0].Category)
	}

	if !stringsContains(facts[0].Content, "symaira projekt") {
		t.Errorf("expected content to contain 'symaira projekt', got '%s'", facts[0].Content)
	}
}

func TestPatternExtraction_IncludesGroundedEvidence(t *testing.T) {
	pe := NewPatternExtractor()
	input := "I like TypeScript."
	facts := pe.ExtractFacts(input)

	if len(facts) == 0 {
		t.Fatalf("expected at least one fact")
	}

	f := facts[0]
	if len(f.Evidence) != 1 {
		t.Fatalf("expected exactly 1 evidence span, got %d", len(f.Evidence))
	}
	ev := f.Evidence[0]
	if ev.AlignmentStatus != evidencekit.AlignmentExact {
		t.Errorf("expected exact alignment, got %s", ev.AlignmentStatus)
	}
	if err := evidencekit.Validate(ev); err != nil {
		t.Errorf("expected valid grounded extraction, got error: %v", err)
	}
	got := input[ev.Span.Start:ev.Span.End]
	if got != ev.EvidenceText {
		t.Errorf("span %v does not slice back to evidence text: got %q, want %q", ev.Span, got, ev.EvidenceText)
	}
}

func TestKeywordFilterFallback_IncludesGroundedEvidence(t *testing.T) {
	pe := NewPatternExtractor()
	input := "Wir planen das symaira projekt aufzubauen."
	facts := pe.ExtractFacts(input)

	if len(facts) == 0 {
		t.Fatalf("expected keyword fallback fact to be extracted")
	}
	ev := facts[0].Evidence
	if len(ev) != 1 {
		t.Fatalf("expected exactly 1 evidence span, got %d", len(ev))
	}
	if err := evidencekit.Validate(ev[0]); err != nil {
		t.Errorf("expected valid grounded extraction, got error: %v", err)
	}
	got := input[ev[0].Span.Start:ev[0].Span.End]
	if got != ev[0].EvidenceText {
		t.Errorf("span %v does not slice back to evidence text: got %q, want %q", ev[0].Span, got, ev[0].EvidenceText)
	}
}

func stringsContains(s, sub string) bool {
	for i := 0; i <= len(s)-len(sub); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}
