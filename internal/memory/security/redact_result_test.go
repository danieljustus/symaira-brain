package security

import (
	"bytes"
	"log/slog"
	"strings"
	"testing"
)

func TestRedactWithResult_LabelsAndPreviews(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name        string
		input       string
		wantLabels  []string // non-empty — expected pattern labels (order-independent)
		wantPreview string   // substring expected in at least one fingerprint preview
	}{
		{
			name:        "email address",
			input:       "user@example.com",
			wantLabels:  []string{LabelEmail},
			wantPreview: ".com", // last 4 chars ".com" should appear at end of preview
		},
		{
			name:        "OpenAI project key",
			input:       "sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ123456",
			wantLabels:  []string{LabelOpenAIProjectKey},
			wantPreview: "sk-p...3456", // first 4 'sk-p' wait, that's only 4 chars? 'sk-p' is 4. last 4 '3456'
		},
		{
			name:        "credit card with Luhn",
			input:       "4111111111111111",
			wantLabels:  []string{LabelCreditCard},
			wantPreview: "4111...1111",
		},
		{
			name:        "GitHub token",
			input:       "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
			wantLabels:  []string{LabelGitHubToken},
			wantPreview: "ghp_...6789",
		},
		{
			name:        "URL credential",
			input:       "https://user:p4ss@host.com/path",
			wantLabels:  []string{LabelURLCredential},
			wantPreview: "http...path",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, result := pg.RedactWithResult(tt.input)

			if len(result.Matches) == 0 {
				t.Fatal("expected at least one match, got none")
			}

			// Check that all wanted labels appear somewhere in the matches.
			seenLabels := make(map[string]bool)
			for _, m := range result.Matches {
				seenLabels[m.PatternLabel] = true
			}
			for _, want := range tt.wantLabels {
				if !seenLabels[want] {
					t.Errorf("expected pattern label %q in matches, got %v", want, seenLabels)
				}
			}

			// Check that at least one preview contains the expected substring.
			found := false
			for _, m := range result.Matches {
				if strings.Contains(m.FingerprintedPreview, tt.wantPreview) {
					found = true
					break
				}
			}
			if !found {
				t.Errorf("expected a preview containing %q, got %v", tt.wantPreview, result.Matches)
			}

			// Safety: ensure no raw value leaks into the result.
			for _, m := range result.Matches {
				if strings.Contains(m.FingerprintedPreview, tt.input) {
					t.Errorf("raw matched value leaked into fingerprinted preview: %q", m.FingerprintedPreview)
				}
			}
		})
	}
}

func TestRedactWithResult_ShortValuesFullyMasked(t *testing.T) {
	pg := NewPIIGuard()

	// For values ≤ 8 chars that match the email pattern.
	// Short emails like a@b.co (7 chars) should be fully masked.
	result, rr := pg.RedactWithResult("contact a@b.co")
	if len(rr.Matches) == 0 {
		t.Fatal("expected match for short email")
	}
	for _, m := range rr.Matches {
		if m.FingerprintedPreview != "[REDACTED]" {
			t.Errorf("expected fully masked preview for short value, got %q", m.FingerprintedPreview)
		}
	}
	if result != "contact [REDACTED_EMAIL]" {
		t.Errorf("expected redacted output, got %q", result)
	}
}

func TestRedactWithResult_MultipleMatches(t *testing.T) {
	pg := NewPIIGuard()

	input := "email: alice@example.com, key: sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"
	_, result := pg.RedactWithResult(input)

	if len(result.Matches) < 2 {
		t.Fatalf("expected at least 2 matches, got %d: %+v", len(result.Matches), result.Matches)
	}

	foundEmail := false
	foundAPIKey := false
	for _, m := range result.Matches {
		if m.PatternLabel == LabelEmail {
			foundEmail = true
		}
		if m.PatternLabel == LabelOpenAIProjectKey {
			foundAPIKey = true
		}
	}
	if !foundEmail {
		t.Error("expected email pattern label in matches")
	}
	if !foundAPIKey {
		t.Error("expected openai_project_key pattern label in matches")
	}
}

func TestRedactWithResult_NoRawValueInSlogOutput(t *testing.T) {
	// Capture slog output to verify raw values never appear.
	var buf bytes.Buffer
	handler := slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelWarn})
	slog.SetDefault(slog.New(handler))
	defer slog.SetDefault(slog.Default()) // restore

	pg := NewPIIGuard()
	sensitive := "my-secret-api-key-1234567890"

	_, result := pg.RedactWithResult("token = " + sensitive)

	if len(result.Matches) == 0 {
		t.Fatal("expected at least one match")
	}

	// Simulate the pattern of logging done in prep.go: log with slog.Warn
	for _, m := range result.Matches {
		slog.Warn("PII redacted", "pattern", m.PatternLabel)
	}

	output := buf.String()

	// The raw value must NEVER appear in the log.
	if strings.Contains(output, sensitive) {
		t.Errorf("raw matched value leaked into slog output:\n%s", output)
	}

	// The pattern label SHOULD appear.
	if !strings.Contains(output, LabelGenericAPIKey) && !strings.Contains(output, LabelHighEntropySecret) {
		t.Errorf("expected pattern label in slog output for match, got:\n%s", output)
	}

	// The fingerprinted preview should only appear in the result struct,
	// not in the slog output (since prep.go only logs the PatternLabel).
	if strings.Contains(output, "...") {
		// Fingerprint pattern contains "..." but that might be OK in structured logging.
		// Be more specific: check if the actual preview text leaked.
		// The preview for 'my-secret-api-key-1234567890' (len=28) is 'my-s...7890'
		// This shouldn't be in the slog output.
		if strings.Contains(output, "my-s...7890") {
			t.Errorf("fingerprinted preview should not appear in slog output:\n%s", output)
		}
	}
}

func TestRedactWithResult_NoMatches(t *testing.T) {
	pg := NewPIIGuard()

	input := "this is clean text with no PII"
	cleaned, result := pg.RedactWithResult(input)

	if cleaned != input {
		t.Errorf("expected unchanged text, got %q", cleaned)
	}
	if len(result.Matches) != 0 {
		t.Errorf("expected no matches, got %d", len(result.Matches))
	}
}

// TestRedact_StillWorks verifies the existing Redact() API is unchanged.
func TestRedact_StillWorks(t *testing.T) {
	input := "email is test@domain.com, sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"
	got := Redact(input)

	if strings.Contains(got, "test@domain.com") {
		t.Errorf("email not redacted: %q", got)
	}
	if !strings.Contains(got, "[REDACTED_API_KEY]") {
		t.Errorf("expected [REDACTED_API_KEY] in output, got %q", got)
	}
	if !strings.Contains(got, "[REDACTED_EMAIL]") {
		t.Errorf("expected [REDACTED_EMAIL] in output, got %q", got)
	}
}

func TestRedactWithResult_FingerprintBoundary(t *testing.T) {
	// 8-char value should be fully masked.
	if got := fingerprint("12345678"); got != "[REDACTED]" {
		t.Errorf("expected [REDACTED] for 8-char value, got %q", got)
	}
	// 9-char value should get partial fingerprint.
	if got := fingerprint("123456789"); got != "1234...6789" {
		t.Errorf("expected 1234...6789 for 9-char value, got %q", got)
	}
	// 4-char value → masked.
	if got := fingerprint("abcd"); got != "[REDACTED]" {
		t.Errorf("expected [REDACTED] for 4-char value, got %q", got)
	}
	// Empty string → masked.
	if got := fingerprint(""); got != "[REDACTED]" {
		t.Errorf("expected [REDACTED] for empty string, got %q", got)
	}
}
