package temporal

import (
	"testing"
	"time"
)

func TestExtract(t *testing.T) {
	// Fixed reference time: 2026-07-30 12:00:00 UTC (Thursday).
	now := time.Date(2026, 7, 30, 12, 0, 0, 0, time.UTC)

	tests := []struct {
		name      string
		query     string
		wantFrom  string // expected From in YYYY-MM-DD or empty for nil
		wantTo    string // expected To in YYYY-MM-DD or empty for nil
		wantMatch string // expected MatchedText substring or empty for nil result
		wantConf  string // expected confidence or empty for nil result
		wantNil   bool   // true when nil result expected
	}{
		// --- English named periods ---
		{
			name:      "yesterday",
			query:     "yesterday",
			wantFrom:  "2026-07-29",
			wantTo:    "2026-07-30",
			wantMatch: "yesterday",
			wantConf:  "high",
		},
		{
			name:      "today",
			query:     "today",
			wantFrom:  "2026-07-30",
			wantTo:    "2026-07-31",
			wantMatch: "today",
			wantConf:  "high",
		},
		{
			name:      "last week",
			query:     "last week",
			wantFrom:  "2026-07-20", // Monday of previous week (Mon 2026-07-20)
			wantTo:    "2026-07-27",
			wantMatch: "last week",
			wantConf:  "high",
		},
		{
			name:      "this week",
			query:     "this week",
			wantFrom:  "2026-07-27", // Monday of current week (Mon 2026-07-27)
			wantTo:    "2026-08-03",
			wantMatch: "this week",
			wantConf:  "high",
		},
		{
			name:      "last month",
			query:     "last month",
			wantFrom:  "2026-06-01",
			wantTo:    "2026-07-01",
			wantMatch: "last month",
			wantConf:  "high",
		},
		{
			name:      "this month",
			query:     "this month",
			wantFrom:  "2026-07-01",
			wantTo:    "2026-08-01",
			wantMatch: "this month",
			wantConf:  "high",
		},
		{
			name:      "last year",
			query:     "last year",
			wantFrom:  "2025-01-01",
			wantTo:    "2026-01-01",
			wantMatch: "last year",
			wantConf:  "high",
		},
		{
			name:      "this year",
			query:     "this year",
			wantFrom:  "2026-01-01",
			wantTo:    "2027-01-01",
			wantMatch: "this year",
			wantConf:  "high",
		},
		// --- English N-duration expressions ---
		{
			name:      "last 3 days",
			query:     "in the last 3 days",
			wantFrom:  "2026-07-27",
			wantTo:    "", // no To bound
			wantMatch: "in the last 3 days",
			wantConf:  "high",
		},
		{
			name:      "past 2 weeks",
			query:     "in the past 2 weeks",
			wantFrom:  "2026-07-16",
			wantTo:    "",
			wantMatch: "in the past 2 weeks",
			wantConf:  "high",
		},
		{
			name:      "last 1 month",
			query:     "in the last 1 month",
			wantFrom:  "2026-06-30",
			wantTo:    "",
			wantMatch: "in the last 1 month",
			wantConf:  "high",
		},
		{
			name:      "last 2 years",
			query:     "in the last 2 years",
			wantFrom:  "2024-07-30",
			wantTo:    "",
			wantMatch: "in the last 2 years",
			wantConf:  "high",
		},
		// --- English "N ago" expressions ---
		{
			name:      "5 days ago",
			query:     "5 days ago",
			wantFrom:  "2026-07-25",
			wantTo:    "",
			wantMatch: "5 days ago",
			wantConf:  "high",
		},
		{
			name:      "3 weeks ago",
			query:     "3 weeks ago",
			wantFrom:  "2026-07-09",
			wantTo:    "",
			wantMatch: "3 weeks ago",
			wantConf:  "high",
		},
		// --- German expressions ---
		{
			name:      "gestern",
			query:     "gestern",
			wantFrom:  "2026-07-29",
			wantTo:    "2026-07-30",
			wantMatch: "gestern",
			wantConf:  "high",
		},
		{
			name:      "heute",
			query:     "heute",
			wantFrom:  "2026-07-30",
			wantTo:    "2026-07-31",
			wantMatch: "heute",
			wantConf:  "high",
		},
		{
			name:      "letzte Woche",
			query:     "letzte Woche",
			wantFrom:  "2026-07-20",
			wantTo:    "2026-07-27",
			wantMatch: "letzte Woche",
			wantConf:  "high",
		},
		{
			name:      "letzten Monat",
			query:     "letzten Monat",
			wantFrom:  "2026-06-01",
			wantTo:    "2026-07-01",
			wantMatch: "letzten Monat",
			wantConf:  "high",
		},
		{
			name:      "letztes Jahr",
			query:     "letztes Jahr",
			wantFrom:  "2025-01-01",
			wantTo:    "2026-01-01",
			wantMatch: "letztes Jahr",
			wantConf:  "high",
		},
		{
			name:      "diese Woche",
			query:     "diese Woche",
			wantFrom:  "2026-07-27",
			wantTo:    "2026-08-03",
			wantMatch: "diese Woche",
			wantConf:  "high",
		},
		{
			name:      "diesen Monat",
			query:     "diesen Monat",
			wantFrom:  "2026-07-01",
			wantTo:    "2026-08-01",
			wantMatch: "diesen Monat",
			wantConf:  "high",
		},
		{
			name:      "dieses Jahr",
			query:     "dieses Jahr",
			wantFrom:  "2026-01-01",
			wantTo:    "2027-01-01",
			wantMatch: "dieses Jahr",
			wantConf:  "high",
		},
		{
			name:      "in den letzten 3 Tagen",
			query:     "in den letzten 3 Tagen",
			wantFrom:  "2026-07-27",
			wantTo:    "",
			wantMatch: "in den letzten 3 Tagen",
			wantConf:  "high",
		},
		{
			name:      "vor 5 Tagen",
			query:     "vor 5 Tagen",
			wantFrom:  "2026-07-25",
			wantTo:    "",
			wantMatch: "vor 5 Tagen",
			wantConf:  "high",
		},
		{
			name:      "vor 2 Wochen",
			query:     "vor 2 Wochen",
			wantFrom:  "2026-07-16",
			wantTo:    "",
			wantMatch: "vor 2 Wochen",
			wantConf:  "high",
		},
		{
			name:      "vor 3 Monaten",
			query:     "vor 3 Monaten",
			wantFrom:  "2026-05-01",
			wantTo:    "",
			wantMatch: "vor 3 Monaten",
			wantConf:  "high",
		},
		{
			name:      "vor 1 Jahr",
			query:     "vor 1 Jahr",
			wantFrom:  "2025-07-30",
			wantTo:    "",
			wantMatch: "vor 1 Jahr",
			wantConf:  "high",
		},
		// --- Mixed in longer query ---
		{
			name:      "yesterday in context",
			query:     "database config yesterday migration",
			wantFrom:  "2026-07-29",
			wantTo:    "2026-07-30",
			wantMatch: "yesterday",
			wantConf:  "high",
		},
		{
			name:      "german in context",
			query:     "Server Konfiguration letzte Woche geändert",
			wantFrom:  "2026-07-20",
			wantTo:    "2026-07-27",
			wantMatch: "letzte Woche",
			wantConf:  "high",
		},
		// --- Negative cases: no temporal signal ---
		{
			name:    "empty string",
			query:   "",
			wantNil: true,
		},
		{
			name:    "no temporal expression",
			query:   "database configuration settings",
			wantNil: true,
		},
		{
			name:    "weak temporal signal ignored",
			query:   "soon",
			wantNil: true,
		},
		{
			name:    "generic future reference ignored",
			query:   "next week",
			wantNil: true,
		},
		{
			name:    "tomorrow ignored",
			query:   "tomorrow",
			wantNil: true,
		},
		{
			name:    "german weak signal ignored",
			query:   "bald",
			wantNil: true,
		},
		{
			name:    "german future reference ignored",
			query:   "nächste Woche",
			wantNil: true,
		},
		{
			name:    "nicht temporal",
			query:   "letztes mal",
			wantNil: true,
		},
		{
			name:    "german morgen ignored (tomorrow not yesterday)",
			query:   "morgen",
			wantNil: true,
		},
		{
			name:    "single letter",
			query:   "a",
			wantNil: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Extract(tt.query, now)
			if tt.wantNil {
				if got != nil {
					t.Errorf("Extract(%q) = %+v, want nil", tt.query, got)
				}
				return
			}
			if got == nil {
				t.Fatalf("Extract(%q) = nil, want non-nil", tt.query)
			}
			if got.Confidence != tt.wantConf {
				t.Errorf("Extract(%q).Confidence = %q, want %q", tt.query, got.Confidence, tt.wantConf)
			}
			if !stringContains(got.MatchedText, tt.wantMatch) {
				t.Errorf("Extract(%q).MatchedText = %q, want substring %q", tt.query, got.MatchedText, tt.wantMatch)
			}
			checkTimeField(t, "From", got.Window.From, tt.wantFrom)
			checkTimeField(t, "To", got.Window.To, tt.wantTo)
		})
	}
}

func checkTimeField(t *testing.T, field string, got *time.Time, want string) {
	t.Helper()
	if want == "" {
		if got != nil {
			t.Errorf("%s = %v, want nil", field, *got)
		}
		return
	}
	if got == nil {
		t.Errorf("%s = nil, want %s", field, want)
		return
	}
	gotStr := got.Format("2006-01-02")
	if gotStr != want {
		t.Errorf("%s = %s, want %s", field, gotStr, want)
	}
}

func stringContains(s, substr string) bool {
	return len(s) >= len(substr) && contains(s, substr)
}

func contains(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

func TestExtract_NowDefault(t *testing.T) {
	// Verify that empty now is handled (uses zero time).
	// Should not panic.
	defer func() {
		if r := recover(); r != nil {
			t.Fatalf("Extract with zero now panicked: %v", r)
		}
	}()
	result := Extract("yesterday", time.Time{})
	if result == nil {
		t.Error("expected non-nil result for yesterday even with zero now")
	}
}

// TestExtract_CaseInsensitive verifies that case doesn't matter.
func TestExtract_CaseInsensitive(t *testing.T) {
	now := time.Date(2026, 7, 30, 12, 0, 0, 0, time.UTC)

	tests := []struct {
		query string
		match string
	}{
		{"YESTERDAY", "YESTERDAY"},
		{"Last Week", "Last Week"},
		{"Gestern", "Gestern"},
		{"Letzte Woche", "Letzte Woche"},
	}

	for _, tt := range tests {
		t.Run(tt.query, func(t *testing.T) {
			result := Extract(tt.query, now)
			if result == nil {
				t.Fatalf("Extract(%q) = nil, want non-nil", tt.query)
			}
			if !stringContains(result.MatchedText, tt.match) {
				t.Errorf("MatchedText = %q, want %q", result.MatchedText, tt.match)
			}
		})
	}
}
