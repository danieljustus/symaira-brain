package summarizer

import (
	"strings"
	"testing"
)

func TestSessionSummarization(t *testing.T) {
	summarizer := NewExtractiveSummarizer()

	dialogue := `
User: Hello! I want to start a new project.
Assistant: Hi! What kind of project is it?
User: I am building symaira-memory today.
Assistant: That sounds interesting! What tools will you use?
User: I will use Go, SQLite, and a custom TUI built with Bubble Tea.
Assistant: Excelent choice!
User: The goal is to finish the core Phase 1.
`

	summary := summarizer.SummarizeSession(dialogue, 3)

	if !strings.Contains(summary, "Session Context Summary:") {
		t.Errorf("summary missing expected header")
	}

	// Verify that conversational noise is excluded, and high-value project facts are captured
	if strings.Contains(strings.ToLower(summary), "hello") {
		t.Errorf("summary should not contain greeting noise")
	}

	// Should extract the core details
	if !strings.Contains(strings.ToLower(summary), "symaira-memory") {
		t.Errorf("summary failed to extract the key project topic 'symaira-memory'")
	}

	if !strings.Contains(strings.ToLower(summary), "bubble tea") {
		t.Errorf("summary failed to extract the key technology 'bubble tea'")
	}
}
