package memory

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestReceiptFormat(t *testing.T) {
	now := time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC)
	m := &db.Memory{
		ID:        "mem-1",
		Content:   "The daemon runs on port 8787.",
		Scope:     "project",
		CreatedAt: now.Add(-72 * time.Hour),
	}

	got := Receipt(m, now)
	want := "◉ memory: \"The daemon runs on port 8787.\" (project, 3d)"
	if got != want {
		t.Errorf("Receipt() = %q, want %q", got, want)
	}
}

func TestReceiptDeterministic(t *testing.T) {
	now := time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC)
	m := &db.Memory{ID: "mem-1", Content: "Fact.", Scope: "global", CreatedAt: now.Add(-1 * time.Hour)}
	a := Receipt(m, now)
	b := Receipt(m, now)
	if a != b {
		t.Errorf("Receipt() not deterministic: %q vs %q", a, b)
	}
}

func TestReceiptTruncatesAndNormalizesContent(t *testing.T) {
	now := time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC)
	long := strings.Repeat("x", 200)
	m := &db.Memory{ID: "mem-1", Content: long, Scope: "global", CreatedAt: now}

	got := Receipt(m, now)
	if len(got) > 100 {
		t.Errorf("receipt too long: %d chars", len(got))
	}
	// The 61st rune is cut off, not the whole content quoted.
	if strings.Contains(got, strings.Repeat("x", 61)) {
		t.Error("receipt did not truncate the content prefix")
	}

	// Newlines collapse to spaces; double quotes are neutralized so the
	// receipt stays a valid one-liner.
	m.Content = "line one\nline two with \"quotes\""
	got = Receipt(m, now)
	if strings.Contains(got, "\n") {
		t.Errorf("receipt contains a newline: %q", got)
	}
	if strings.Contains(got, `"quotes"`) {
		t.Errorf("receipt contains raw double quotes: %q", got)
	}
}

func TestReceiptAgeRendering(t *testing.T) {
	now := time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC)
	cases := []struct {
		name    string
		created time.Time
		wantAge string
	}{
		{"just now", now.Add(-30 * time.Second), "just now"},
		{"minutes", now.Add(-5 * time.Minute), "5m"},
		{"hours", now.Add(-3 * time.Hour), "3h"},
		{"days", now.Add(-48 * time.Hour), "2d"},
		{"weeks", now.Add(-21 * 24 * time.Hour), "21d"},
		{"unknown", time.Time{}, "unknown"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			m := &db.Memory{ID: "mem-1", Content: "fact", Scope: "global", CreatedAt: tc.created}
			got := Receipt(m, now)
			if !strings.Contains(got, "("+m.Scope+", "+tc.wantAge+")") {
				t.Errorf("receipt age %q: got %q", tc.wantAge, got)
			}
		})
	}
}

func TestReceiptNilMemory(t *testing.T) {
	if got := Receipt(nil, time.Now()); got != "" {
		t.Errorf("Receipt(nil) = %q, want empty", got)
	}
}
