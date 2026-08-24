package usage

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestNousParsesAccountResponse(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("nous-account.json"), nil)
	strategy := &nousPortalAPIStrategy{
		accessToken:   "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig",
		portalBaseURL: nousDefaultPortalURL,
		client:        client,
	}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "nous" {
		t.Errorf("ProviderID = %q, want nous", snap.ProviderID)
	}
	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	if snap.Balance == nil || *snap.Balance != "57.5" {
		t.Errorf("Balance = %v, want 57.5", snap.Balance)
	}
	if m := meterByLabel(snap.Meters, "Subscription credits"); m == nil || m.Used == nil || *m.Used != "42.5" {
		t.Errorf("Subscription credits = %v, want used=42.5", m)
	}
	if m := meterByLabel(snap.Meters, "Purchased credits"); m == nil || m.Used == nil || *m.Used != "15" {
		t.Errorf("Purchased credits = %v, want used=15", m)
	}
	if m := meterByLabel(snap.Meters, "Plan credits remaining"); m == nil || m.Limit == nil || *m.Limit != "100" {
		t.Errorf("Plan credits remaining = %v, want limit=100", m)
	}
	if meterByLabel(snap.Meters, "Rollover credits") == nil {
		t.Error("expected a Rollover credits meter")
	}
}

func TestNousProviderUnconfiguredWithoutToken(t *testing.T) {
	p := &NousPortalProvider{accessToken: "", portalBaseURL: nousDefaultPortalURL}
	if p.IsConfigured() {
		t.Error("expected unconfigured without a token")
	}
	if len(p.Strategies()) != 0 {
		t.Error("expected no strategies when unconfigured")
	}
}

func TestNousProviderConfiguredWithToken(t *testing.T) {
	p := &NousPortalProvider{accessToken: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig", portalBaseURL: nousDefaultPortalURL}
	if !p.IsConfigured() {
		t.Error("expected configured with a token")
	}
}

// MARK: Auth store (strictly read-only)

func base64URLEncode(v map[string]any) string {
	data, _ := json.Marshal(v)
	return base64.RawURLEncoding.EncodeToString(data)
}

func writeNousAuthStore(t *testing.T, body string) string {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "auth.json")
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}
	return path
}

func TestNousAuthStoreReadsInvokeJWTFromFixtures(t *testing.T) {
	future := time.Now().Add(time.Hour).Unix()
	payload := base64URLEncode(map[string]any{"exp": future})
	token := fmt.Sprintf("eyJhbGciOiJub25lIn0.%s.sig", payload)
	store := fmt.Sprintf(`{"version":1,"providers":[{"id":"nous","invoke_jwt":%q,"client_id":"hermes-cli"}]}`, token)
	path := writeNousAuthStore(t, store)

	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile(): %v", err)
	}
	got := readNousAccessToken(path)
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile(): %v", err)
	}

	if got != token {
		t.Errorf("readNousAccessToken() = %q, want %q", got, token)
	}
	if string(before) != string(after) {
		t.Error("auth.json must stay byte-identical after a read")
	}
}

func TestNousAuthStoreTreatsExpiredJWTAsUnavailable(t *testing.T) {
	past := time.Now().Add(-time.Hour).Unix()
	payload := base64URLEncode(map[string]any{"exp": past})
	token := fmt.Sprintf("eyJhbGciOiJub25lIn0.%s.sig", payload)
	store := fmt.Sprintf(`{"version":1,"providers":[{"id":"nous","invoke_jwt":%q,"client_id":"hermes-cli"}]}`, token)
	path := writeNousAuthStore(t, store)

	if got := readNousAccessToken(path); got != "" {
		t.Errorf("readNousAccessToken() = %q, want empty — expired token must surface as re-auth needed, never refreshed", got)
	}
}

func TestNousAuthStoreIgnoresOtherProviders(t *testing.T) {
	store := `{"version":1,"providers":[{"id":"opencode","access_token":"sekrit"}]}`
	path := writeNousAuthStore(t, store)

	if got := readNousAccessToken(path); got != "" {
		t.Errorf("readNousAccessToken() = %q, want empty — other providers' tokens must not be used for nous", got)
	}
}

func TestNousAuthStoreMissingFileYieldsEmpty(t *testing.T) {
	if got := readNousAccessToken(filepath.Join(t.TempDir(), "does-not-exist.json")); got != "" {
		t.Errorf("readNousAccessToken() = %q, want empty for a missing file", got)
	}
}

// MARK: Errors never leak token material

func TestNousAuthErrorSurfacesReAuthHintWithoutTokenMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &nousPortalAPIStrategy{
		accessToken:   "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzZWNyZXQifQ.sig-secret-material",
		portalBaseURL: nousDefaultPortalURL,
		client:        client,
	}

	_, err := strategy.Fetch(context.Background())
	var notConfigured *NotConfiguredError
	if !asNotConfigured(err, &notConfigured) {
		t.Fatalf("expected *NotConfiguredError, got %v (%T)", err, err)
	}
	if notConfigured.ProviderID != "nous" {
		t.Errorf("ProviderID = %q, want nous", notConfigured.ProviderID)
	}
	if strings.Contains(err.Error(), "sig-secret") {
		t.Error("token material must never leak into the error")
	}
}

func TestNousAuthorizationHeaderUsesBearerToken(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("nous-account.json"), nil)
	strategy := &nousPortalAPIStrategy{
		accessToken:   "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig",
		portalBaseURL: nousDefaultPortalURL,
		client:        client,
	}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig" {
		t.Errorf("Authorization = %q, want the bearer token", got)
	}
	if got := transport.lastRequest.URL.Path; got != "/api/oauth/account" {
		t.Errorf("path = %q, want /api/oauth/account", got)
	}
}

// MARK: 429 -> RateLimitedError

func TestNousHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "20"})
	strategy := &nousPortalAPIStrategy{
		accessToken:   "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig",
		portalBaseURL: nousDefaultPortalURL,
		client:        client,
	}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "nous" {
		t.Errorf("ProviderID = %q, want nous", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 20 {
		t.Errorf("RetryAfterSeconds = %v, want 20", rateLimited.RetryAfterSeconds)
	}
}

func TestNousHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &nousPortalAPIStrategy{
		accessToken:   "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig",
		portalBaseURL: nousDefaultPortalURL,
		client:        client,
	}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
