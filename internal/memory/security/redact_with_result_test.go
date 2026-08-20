package security

import (
	"strings"
	"testing"
)

// TestRedactWithResultNilMap covers the nil-map guard of RedactMapWithResult
// (RedactWithResult itself is covered in redact_result_test.go).
func TestRedactWithResultNilMap(t *testing.T) {
	got, rr := RedactMapWithResult(nil)
	if got != nil {
		t.Errorf("nil map should return nil, got %v", got)
	}
	if rr.Matches != nil {
		t.Errorf("nil map should return empty result, got %v", rr.Matches)
	}
}

// TestRedactMapWithResultValues covers the map variant: values redacted,
// matches aggregated, original map untouched.
func TestRedactMapWithResultValues(t *testing.T) {
	orig := map[string]string{
		"email": "boss@example.com",
		"note":  "plain note",
	}
	got, rr := RedactMapWithResult(orig)
	if len(got) != 2 {
		t.Fatalf("expected 2 keys, got %d", len(got))
	}
	if got["email"] == "boss@example.com" {
		t.Errorf("email value not redacted: %q", got["email"])
	}
	if got["note"] != "plain note" {
		t.Errorf("plain value changed: %q", got["note"])
	}
	if orig["email"] != "boss@example.com" {
		t.Errorf("original map mutated: %q", orig["email"])
	}
	if len(rr.Matches) == 0 {
		t.Error("expected aggregated matches from map redaction")
	}
}

// TestRedactWithResultCleanText covers the no-match path of
// RedactWithResult via the package-level singleton.
func TestRedactWithResultCleanText(t *testing.T) {
	redacted, result := RedactWithResult("no secrets here")
	if redacted != "no secrets here" {
		t.Errorf("clean text changed: %q", redacted)
	}
	if len(result.Matches) != 0 {
		t.Errorf("expected no matches for clean text, got %d", len(result.Matches))
	}

	input := "Contact alice@example.com or call 0170 1234567 today."
	redacted, result = RedactWithResult(input)
	if strings.Contains(redacted, "alice@example.com") {
		t.Errorf("email not redacted: %q", redacted)
	}
	if len(result.Matches) == 0 {
		t.Fatal("expected at least one redaction match")
	}
	for _, m := range result.Matches {
		if m.PatternLabel == "" {
			t.Error("match without pattern label")
		}
		if strings.Contains(m.FingerprintedPreview, "alice@example.com") {
			t.Errorf("preview leaks raw value: %q", m.FingerprintedPreview)
		}
	}
}
