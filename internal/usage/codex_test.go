package usage

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCodexParsesWhamUsage(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("codex-wham-usage.json"), nil)
	strategy := &codexOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "codex" {
		t.Errorf("ProviderID = %q, want codex", snap.ProviderID)
	}
	if snap.Source != "oauth" {
		t.Errorf("Source = %q, want oauth", snap.Source)
	}
	primary := meterByLabel(snap.Meters, "5h")
	secondary := meterByLabel(snap.Meters, "1w")
	if primary == nil || primary.Used == nil || *primary.Used != "25" {
		t.Errorf("primary meter = %v, want used=25", primary)
	}
	if primary == nil || primary.Limit == nil || *primary.Limit != "100" {
		t.Errorf("primary meter = %v, want limit=100", primary)
	}
	if primary == nil || primary.Unit != "%" {
		t.Errorf("primary unit = %v, want %%", primary)
	}
	if primary == nil || primary.ResetsAt == nil {
		t.Error("expected primary.ResetsAt to be set")
	}
	if secondary == nil || secondary.Used == nil || *secondary.Used != "200" {
		t.Errorf("secondary meter = %v, want used=200", secondary)
	}
	if secondary == nil || secondary.Limit == nil || *secondary.Limit != "800" {
		t.Errorf("secondary meter = %v, want limit=800", secondary)
	}
	if meterByLabel(snap.Meters, "Codex Spark Weekly") == nil {
		t.Error("expected a Codex Spark Weekly meter")
	}
}

func TestCodexProviderUnconfiguredWithoutToken(t *testing.T) {
	p := &CodexProvider{accessToken: "", homeDir: "/nonexistent"}
	if p.IsConfigured() {
		t.Error("expected unconfigured without a token")
	}
}

func TestCodexProviderConfiguredWithToken(t *testing.T) {
	p := &CodexProvider{accessToken: "sk-test", homeDir: "/nonexistent"}
	if !p.IsConfigured() {
		t.Error("expected configured with a token")
	}
}

// MARK: Auth store (strictly read-only)

func TestCodexAuthStoreReadsAccessToken(t *testing.T) {
	dir := t.TempDir()
	store := `{"access_token": "sk-ant-oat-synthetic", "openai_login": {"email": "dev@example.com", "plan": "pro"}}`
	path := filepath.Join(dir, "auth.json")
	if err := os.WriteFile(path, []byte(store), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	before, _ := os.ReadFile(path)
	token := readCodexAccessToken(dir)
	after, _ := os.ReadFile(path)

	if token != "sk-ant-oat-synthetic" {
		t.Errorf("token = %q, want sk-ant-oat-synthetic", token)
	}
	if string(before) != string(after) {
		t.Error("auth.json must stay byte-identical after a read")
	}
}

func TestCodexAuthStoreFallsBackToNestedTokens(t *testing.T) {
	dir := t.TempDir()
	store := `{"tokens": {"access_token": "sk-nested"}}`
	if err := os.WriteFile(filepath.Join(dir, "auth.json"), []byte(store), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	if got := readCodexAccessToken(dir); got != "sk-nested" {
		t.Errorf("token = %q, want sk-nested", got)
	}
}

func TestCodexAuthStoreIgnoresMissingFile(t *testing.T) {
	if got := readCodexAccessToken("/nonexistent-home"); got != "" {
		t.Errorf("token = %q, want empty for a missing file", got)
	}
}

// MARK: Errors never leak token material

func TestCodexAuthErrorIsUnderstandableWithoutTokenMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &codexOAuthStrategy{accessToken: "sk-ant-oat-secret-token-material", client: client}

	_, err := strategy.Fetch(context.Background())
	var notConfigured *NotConfiguredError
	if !asNotConfigured(err, &notConfigured) {
		t.Fatalf("expected *NotConfiguredError, got %v (%T)", err, err)
	}
	if notConfigured.ProviderID != "codex" {
		t.Errorf("ProviderID = %q, want codex", notConfigured.ProviderID)
	}
	if strings.Contains(err.Error(), "sk-ant") {
		t.Error("token material must never leak into the error")
	}
}

func TestCodexAuthorizationHeaderUsesBearerToken(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("codex-wham-usage.json"), nil)
	strategy := &codexOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer sk-ant-oat-test" {
		t.Errorf("Authorization = %q, want Bearer sk-ant-oat-test", got)
	}
	if got := transport.lastRequest.URL.Path; got != "/backend-api/wham/usage" {
		t.Errorf("path = %q, want /backend-api/wham/usage", got)
	}
}

// MARK: 429 -> RateLimitedError

func TestCodexHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "17"})
	strategy := &codexOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "codex" {
		t.Errorf("ProviderID = %q, want codex", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 17 {
		t.Errorf("RetryAfterSeconds = %v, want 17", rateLimited.RetryAfterSeconds)
	}
}

func TestCodexHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &codexOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
