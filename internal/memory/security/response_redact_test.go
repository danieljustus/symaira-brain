package security

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestRedactMemory(t *testing.T) {
	secret := "GitHub auth token is gho_abcdefabcdefabcdefabcdefabcdefabcdef"
	m := &db.Memory{
		ID:      "mem-1",
		Content: secret,
		Metadata: map[string]string{
			"note": secret,
		},
	}

	RedactMemory(m)

	if m.Content == secret {
		t.Fatalf("expected content to be redacted, got raw secret: %q", m.Content)
	}
	if m.Metadata["note"] == secret {
		t.Fatalf("expected metadata to be redacted, got raw secret: %q", m.Metadata["note"])
	}
}

func TestRedactMemoryNil(t *testing.T) {
	// Must not panic on a nil Memory (defensive callers pass results that may be nil).
	RedactMemory(nil)
}

func TestRedactMemories(t *testing.T) {
	secret := "AWS key is AKIA1234567890ABCDEF"
	mems := []*db.Memory{
		{ID: "a", Content: secret},
		nil,
		{ID: "b", Content: "no secret here"},
	}

	RedactMemories(mems)

	if mems[0].Content == secret {
		t.Fatalf("expected mems[0] content redacted, got raw secret: %q", mems[0].Content)
	}
	if mems[2].Content != "no secret here" {
		t.Fatalf("expected unaffected content to survive unchanged, got %q", mems[2].Content)
	}
}

func TestRedactSearchResults(t *testing.T) {
	secret := "API key sk-abcdefghijklmnopqrstuvwxyz"
	results := []db.SearchResult{
		{Memory: &db.Memory{ID: "a", Content: secret}, Score: 0.9},
		{Memory: nil, Score: 0.1},
	}

	RedactSearchResults(results)

	if results[0].Memory.Content == secret {
		t.Fatalf("expected search result content redacted, got raw secret: %q", results[0].Memory.Content)
	}
}
