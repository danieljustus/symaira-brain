package calendar

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewCalendarImporter(t *testing.T) {
	imp := NewCalendarImporter("primary", "", true, 7)

	if imp.Name() != "calendar" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "calendar")
	}

	if imp.Category() != "calendar" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "calendar")
	}

	if imp.PrivacyLevel() != "confidential" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "confidential")
	}

	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}

func TestNewCalendarImporterDefaults(t *testing.T) {
	imp := NewCalendarImporter("", "", false, 0)

	if imp.calendarID != "primary" {
		t.Errorf("calendarID = %q, want %q", imp.calendarID, "primary")
	}

	if imp.includeDesc {
		t.Error("includeDesc = true, want false")
	}

	if imp.includeDays != 7 {
		t.Errorf("includeDays = %d, want %d", imp.includeDays, 7)
	}
}

func TestParseEventTime(t *testing.T) {
	imp := NewCalendarImporter("primary", "", true, 7)

	tests := []struct {
		name     string
		dt       eventDateTime
		wantZero bool
	}{
		{
			name: "datetime",
			dt: eventDateTime{
				DateTime: "2026-06-15T14:00:00+02:00",
			},
			wantZero: false,
		},
		{
			name: "date only (all-day)",
			dt: eventDateTime{
				Date: "2026-06-15",
			},
			wantZero: false,
		},
		{
			name:     "empty",
			dt:       eventDateTime{},
			wantZero: true,
		},
		{
			name: "invalid datetime format",
			dt: eventDateTime{
				DateTime: "not-a-date",
			},
			wantZero: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := imp.parseEventTime(tt.dt)
			if tt.wantZero && !result.IsZero() {
				t.Errorf("parseEventTime() = %v, want zero", result)
			}
			if !tt.wantZero && result.IsZero() {
				t.Error("parseEventTime() = zero, want non-zero")
			}
		})
	}
}

func TestParseToken(t *testing.T) {
	validToken := `{"access_token":"test-token","token_type":"Bearer","expiry":"2026-06-15T14:00:00Z"}`
	token, err := parseToken([]byte(validToken))
	if err != nil {
		t.Fatalf("parseToken() error = %v", err)
	}
	if token.AccessToken != "test-token" {
		t.Errorf("AccessToken = %q, want %q", token.AccessToken, "test-token")
	}
}

func TestParseTokenInvalid(t *testing.T) {
	_, err := parseToken([]byte("not json"))
	if err == nil {
		t.Error("parseToken() with invalid JSON should return error")
	}
}

func TestDiscoverSessionsNoToken(t *testing.T) {
	imp := NewCalendarImporter("primary", "", false, 7)

	_, err := imp.DiscoverSessions(time.Now())
	if err == nil {
		t.Fatal("expected error with no token configured")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
}

func TestImportSessionNoToken(t *testing.T) {
	imp := NewCalendarImporter("primary", "", false, 7)
	ref := importer.SessionRef{SessionID: "event123"}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error with no token configured")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
}

func TestLoadTokenFileNotFound(t *testing.T) {
	imp := NewCalendarImporter("primary", "/nonexistent/file.json", false, 7)

	_, err := imp.loadToken()
	if err == nil {
		t.Fatal("expected error for non-existent token file")
	}
	if !strings.Contains(err.Error(), "read token file") {
		t.Errorf("expected 'read token file' error, got %q", err.Error())
	}
}

func TestLoadTokenInvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	tokenFile := filepath.Join(tmpDir, "token.json")
	if err := os.WriteFile(tokenFile, []byte("not json"), 0644); err != nil {
		t.Fatalf("failed to write token file: %v", err)
	}

	imp := NewCalendarImporter("primary", tokenFile, false, 7)
	_, err := imp.loadToken()
	if err == nil {
		t.Fatal("expected error for invalid token JSON")
	}
	if !strings.Contains(err.Error(), "parse token JSON") {
		t.Errorf("expected 'parse token JSON' error, got %q", err.Error())
	}
}

func TestLoadTokenValidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	tokenFile := filepath.Join(tmpDir, "token.json")
	validToken := `{"access_token":"valid-token","token_type":"Bearer","expiry":"2027-01-01T00:00:00Z"}`
	if err := os.WriteFile(tokenFile, []byte(validToken), 0644); err != nil {
		t.Fatalf("failed to write token file: %v", err)
	}

	imp := NewCalendarImporter("primary", tokenFile, false, 7)
	token, err := imp.loadToken()
	if err != nil {
		t.Fatalf("loadToken() error = %v", err)
	}
	if token.AccessToken != "valid-token" {
		t.Errorf("AccessToken = %q, want %q", token.AccessToken, "valid-token")
	}
}

func TestLoadTokenFromEnv(t *testing.T) {
	tmpDir := t.TempDir()
	tokenFile := filepath.Join(tmpDir, "env_token.json")
	validToken := `{"access_token":"env-token","token_type":"Bearer","expiry":"2027-06-01T00:00:00Z"}`
	if err := os.WriteFile(tokenFile, []byte(validToken), 0644); err != nil {
		t.Fatalf("failed to write token file: %v", err)
	}

	t.Setenv("GOOGLE_CALENDAR_TOKEN", tokenFile)
	imp := NewCalendarImporter("primary", "", false, 7)

	token, err := imp.loadToken()
	if err != nil {
		t.Fatalf("loadToken() from env error = %v", err)
	}
	if token.AccessToken != "env-token" {
		t.Errorf("AccessToken = %q, want %q", token.AccessToken, "env-token")
	}
}

// TestLoadTokenSymVaultUnavailable asserts that the no-token path fails fast
// with the actionable 'no token found' error when the SymVault fallback is
// unavailable, without depending on an ambient symvault process.
func TestLoadTokenSymVaultUnavailable(t *testing.T) {
	t.Setenv("GOOGLE_CALENDAR_TOKEN", "")
	imp := NewCalendarImporter("primary", "", false, 7)

	var invoked bool
	imp.vaultLookup = func(ctx context.Context, path string) ([]byte, error) {
		invoked = true
		if path != "google/calendar-token" {
			t.Errorf("vaultLookup path = %q, want %q", path, "google/calendar-token")
		}
		return nil, fmt.Errorf("exec: symvault: executable file not found in $PATH")
	}

	start := time.Now()
	_, err := imp.loadToken()
	elapsed := time.Since(start)

	if !invoked {
		t.Fatal("vaultLookup was not invoked")
	}
	if err == nil {
		t.Fatal("expected error when SymVault is unavailable")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
	if elapsed > 2*time.Second {
		t.Errorf("loadToken took %v with unavailable SymVault; want fail-fast return", elapsed)
	}
}

// TestLoadTokenSymVaultSlow asserts that a hung SymVault fallback is bounded
// by the context timeout and cannot block the no-token path indefinitely.
func TestLoadTokenSymVaultSlow(t *testing.T) {
	t.Setenv("GOOGLE_CALENDAR_TOKEN", "")
	imp := NewCalendarImporter("primary", "", false, 7)
	imp.vaultTimeout = 100 * time.Millisecond

	var invoked bool
	var sawCancel bool
	imp.vaultLookup = func(ctx context.Context, path string) ([]byte, error) {
		invoked = true
		// Simulate a hung symvault: only return once the bounded context fires.
		<-ctx.Done()
		sawCancel = true
		return nil, ctx.Err()
	}

	start := time.Now()
	_, err := imp.loadToken()
	elapsed := time.Since(start)

	if !invoked {
		t.Fatal("vaultLookup was not invoked")
	}
	if !sawCancel {
		t.Error("bounded context was never cancelled; SymVault fallback is not bounded")
	}
	if err == nil {
		t.Fatal("expected error with hung SymVault")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
	// The stub would block forever if left alone; it must return promptly
	// once the timeout fires.
	if elapsed > 2*time.Second {
		t.Errorf("loadToken took %v with hung SymVault; want bounded return near %v", elapsed, imp.vaultTimeout)
	}
}

// TestDiscoverSessionsNoTokenSymVaultUnavailable asserts the full discovery
// no-token path returns promptly when the SymVault fallback is unavailable.
func TestDiscoverSessionsNoTokenSymVaultUnavailable(t *testing.T) {
	t.Setenv("GOOGLE_CALENDAR_TOKEN", "")
	imp := NewCalendarImporter("primary", "", false, 7)
	imp.vaultLookup = func(ctx context.Context, path string) ([]byte, error) {
		return nil, fmt.Errorf("symvault unavailable")
	}

	start := time.Now()
	_, err := imp.DiscoverSessions(time.Now())
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected error with unavailable SymVault")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
	if elapsed > 2*time.Second {
		t.Errorf("DiscoverSessions took %v with unavailable SymVault; want fail-fast return", elapsed)
	}
}

// TestImportSessionNoTokenSymVaultSlow asserts the full import no-token path
// returns promptly when the SymVault fallback hangs.
func TestImportSessionNoTokenSymVaultSlow(t *testing.T) {
	t.Setenv("GOOGLE_CALENDAR_TOKEN", "")
	imp := NewCalendarImporter("primary", "", false, 7)
	imp.vaultTimeout = 100 * time.Millisecond
	imp.vaultLookup = func(ctx context.Context, path string) ([]byte, error) {
		<-ctx.Done()
		return nil, ctx.Err()
	}
	ref := importer.SessionRef{SessionID: "event123"}

	start := time.Now()
	_, err := imp.ImportSession(ref)
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected error with hung SymVault")
	}
	if !strings.Contains(err.Error(), "no token found") {
		t.Errorf("expected 'no token found' error, got %q", err.Error())
	}
	if elapsed > 2*time.Second {
		t.Errorf("ImportSession took %v with hung SymVault; want bounded return", elapsed)
	}
}

func TestDiscoverSessionsFailsWithBadTokenPath(t *testing.T) {
	imp := NewCalendarImporter("primary", "/nonexistent/token.json", false, 7)

	_, err := imp.DiscoverSessions(time.Now())
	if err == nil {
		t.Fatal("expected error with bad token path")
	}
}

func TestImportSessionFailsWithBadTokenPath(t *testing.T) {
	imp := NewCalendarImporter("primary", "/nonexistent/token.json", false, 7)
	ref := importer.SessionRef{SessionID: "event123"}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error with bad token path")
	}
}

// testTransport rewrites requests bound for googleapis.com to a local test server.
type testTransport struct {
	targetURL string
	inner     http.RoundTripper
}

func (t *testTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	if strings.Contains(req.URL.Host, "googleapis.com") || strings.Contains(req.URL.Host, "www.googleapis.com") {
		newPath := t.targetURL + req.URL.Path
		if req.URL.RawQuery != "" {
			newPath += "?" + req.URL.RawQuery
		}
		newReq, err := http.NewRequest(req.Method, newPath, req.Body)
		if err != nil {
			return nil, err
		}
		newReq.Header = req.Header.Clone()
		return t.inner.RoundTrip(newReq)
	}
	return t.inner.RoundTrip(req)
}

func makeTokenFile(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	f := filepath.Join(dir, "token.json")
	if err := os.WriteFile(f, []byte(`{"access_token":"test","token_type":"Bearer","expiry":"2027-01-01T00:00:00Z"}`), 0644); err != nil {
		t.Fatalf("write token: %v", err)
	}
	return f
}

func withMockGoogleAPI(t *testing.T, handler http.HandlerFunc) func() {
	t.Helper()
	server := httptest.NewServer(handler)
	oldTransport := http.DefaultTransport
	http.DefaultTransport = &testTransport{
		targetURL: server.URL,
		inner:     oldTransport,
	}
	return func() {
		server.Close()
		http.DefaultTransport = oldTransport
	}
}

func TestDiscoverSessionsCancelledEventsSkipped(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"items": [
				{
					"id": "evt-1",
					"summary": "Active Meeting",
					"status": "confirmed",
					"start": {"dateTime": "2026-07-01T10:00:00+02:00"},
					"end": {"dateTime": "2026-07-01T11:00:00+02:00"},
					"organizer": {"email": "user@example.com", "name": "User Name"},
					"attendees": [
						{"email": "alice@example.com", "name": "Alice", "responseStatus": "accepted"},
						{"email": "bob@example.com", "name": "", "responseStatus": "tentative"}
					],
					"location": "Room 1",
					"created": "2026-06-01T10:00:00Z",
					"updated": "2026-06-28T10:00:00Z",
					"htmlLink": "https://calendar.google.com/event?eid=evt-1"
				},
				{
					"id": "evt-2",
					"summary": "Cancelled Event",
					"status": "cancelled",
					"start": {"dateTime": "2026-07-02T14:00:00+02:00"},
					"end": {"dateTime": "2026-07-02T15:00:00+02:00"},
					"organizer": {"email": "user@example.com"},
					"created": "2026-06-01T10:00:00Z",
					"updated": "2026-06-28T10:00:00Z"
				},
				{
					"id": "evt-3",
					"summary": "All Day",
					"status": "confirmed",
					"start": {"date": "2026-07-03"},
					"end": {"date": "2026-07-04"},
					"organizer": {"email": "user@example.com"},
					"location": "",
					"created": "2026-06-01T10:00:00Z",
					"updated": "2026-06-28T10:00:00Z"
				}
			],
			"nextPageToken": ""
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, false, 7)

	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions (cancelled skipped), got %d", len(sessions))
	}
	if sessions[0].SessionID != "evt-1" {
		t.Errorf("first session ID = %q, want %q", sessions[0].SessionID, "evt-1")
	}
	if sessions[0].Metadata["organizer"] != "User Name" {
		t.Errorf("organizer = %q, want %q", sessions[0].Metadata["organizer"], "User Name")
	}
	if sessions[0].Metadata["attendees"] == "" {
		t.Error("expected attendees metadata")
	}
	if !strings.Contains(sessions[0].Metadata["attendees"], "Alice") {
		t.Errorf("expected Alice in attendees, got %q", sessions[0].Metadata["attendees"])
	}
}

func TestDiscoverSessionsRecurringEvents(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"items": [
				{
					"id": "evt-recur",
					"summary": "Daily Standup",
					"status": "confirmed",
					"start": {"dateTime": "2026-07-01T09:00:00+02:00"},
					"end": {"dateTime": "2026-07-01T09:15:00+02:00"},
					"recurringEventId": "recur-parent-id",
					"organizer": {"email": "user@example.com"},
					"created": "2026-06-01T10:00:00Z",
					"updated": "2026-06-28T10:00:00Z"
				}
			],
			"nextPageToken": ""
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, false, 7)

	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}
	if sessions[0].Metadata["recurring"] != "true" {
		t.Errorf("expected recurring=true, got %q", sessions[0].Metadata["recurring"])
	}
}

func TestDiscoverSessionsAllDayEvent(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"items": [
				{
					"id": "all-day",
					"summary": "Vacation",
					"status": "confirmed",
					"start": {"date": "2026-07-04"},
					"end": {"date": "2026-07-10"},
					"organizer": {"email": "user@example.com"},
					"created": "2026-06-01T10:00:00Z",
					"updated": "2026-06-28T10:00:00Z"
				}
			],
			"nextPageToken": ""
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, false, 7)

	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}
	if sessions[0].SessionID != "all-day" {
		t.Errorf("session ID = %q, want %q", sessions[0].SessionID, "all-day")
	}
}

func TestImportSessionFullEvent(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"id": "full-evt",
			"summary": "Project Review",
			"description": "Review Q3 milestones and deliverables.\n\nAgenda:\n1. Status update\n2. Blockers\n3. Next steps",
			"status": "confirmed",
			"start": {"dateTime": "2026-07-15T14:00:00+02:00", "timeZone": "Europe/Berlin"},
			"end": {"dateTime": "2026-07-15T15:30:00+02:00", "timeZone": "Europe/Berlin"},
			"location": "Conference Room B",
			"organizer": {"email": "lead@example.com", "name": "Team Lead"},
			"attendees": [
				{"email": "dev1@example.com", "name": "Dev One", "responseStatus": "accepted"},
				{"email": "dev2@example.com", "name": "Dev Two", "responseStatus": "accepted"}
			],
			"recurringEventId": "parent-123",
			"created": "2026-06-01T10:00:00Z",
			"updated": "2026-07-10T10:00:00Z",
			"htmlLink": "https://calendar.google.com/event?eid=full-evt"
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, true, 7)
	ref := importer.SessionRef{SessionID: "full-evt"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	fact := facts[0]
	if !strings.Contains(fact.Content, "Project Review") {
		t.Errorf("expected content to contain summary, got %q", fact.Content)
	}
	if !strings.Contains(fact.Content, "14:00") {
		t.Errorf("expected content to contain start time, got %q", fact.Content)
	}
	if !strings.Contains(fact.Content, "Conference Room B") {
		t.Errorf("expected content to contain location, got %q", fact.Content)
	}
	if !strings.Contains(fact.Content, "Q3 milestones") {
		t.Errorf("expected content to contain description, got %q", fact.Content)
	}
	if fact.Metadata["start"] == "" {
		t.Error("expected start metadata")
	}
	if fact.Metadata["end"] == "" {
		t.Error("expected end metadata")
	}
	if fact.Metadata["location"] != "Conference Room B" {
		t.Errorf("location = %q, want %q", fact.Metadata["location"], "Conference Room B")
	}
	if fact.Metadata["recurring"] != "true" {
		t.Errorf("recurring = %q, want %q", fact.Metadata["recurring"], "true")
	}
}

func TestImportSessionNoDescription(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"id": "minimal",
			"summary": "Quick Sync",
			"status": "confirmed",
			"start": {"dateTime": "2026-07-01T09:00:00Z"},
			"end": {"dateTime": "2026-07-01T09:30:00Z"},
			"organizer": {"email": "user@example.com"},
			"created": "2026-06-01T10:00:00Z",
			"updated": "2026-06-28T10:00:00Z"
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, false, 7)
	ref := importer.SessionRef{SessionID: "minimal"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if strings.Contains(facts[0].Content, "description") {
		t.Error("expected no description in content when includeDesc=false")
	}
}

func TestImportSessionEventWithZeroDates(t *testing.T) {
	defer withMockGoogleAPI(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"id": "nodate",
			"summary": "Event Without Dates",
			"status": "confirmed",
			"start": {},
			"end": {},
			"organizer": {"email": "user@example.com"},
			"created": "2026-06-01T10:00:00Z",
			"updated": "2026-06-28T10:00:00Z"
		}`))
	})()
	tokenFile := makeTokenFile(t)
	imp := NewCalendarImporter("primary", tokenFile, true, 7)
	ref := importer.SessionRef{SessionID: "nodate"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if strings.Contains(facts[0].Content, " on ") {
		t.Errorf("expected no date for zero start time, got %q", facts[0].Content)
	}
}
