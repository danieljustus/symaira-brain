package usage

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// MARK: OAuth parsing fixtures (no network)

func TestClaudeParsesOAuthUsageWindows(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("claude-oauth-usage.json"), nil)
	strategy := &claudeOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "claude" {
		t.Errorf("ProviderID = %q, want claude", snap.ProviderID)
	}
	if snap.Source != "oauth" {
		t.Errorf("Source = %q, want oauth", snap.Source)
	}
	fiveHour := meterByLabel(snap.Meters, "five_hour")
	if fiveHour == nil || fiveHour.Used == nil || *fiveHour.Used != "45" {
		t.Errorf("five_hour = %v, want used=45", fiveHour)
	}
	if fiveHour == nil || fiveHour.Limit == nil || *fiveHour.Limit != "100" {
		t.Errorf("five_hour = %v, want limit=100", fiveHour)
	}
	if fiveHour == nil || fiveHour.Unit != "%" {
		t.Errorf("five_hour unit = %v, want %%", fiveHour)
	}
	if fiveHour == nil || fiveHour.ResetsAt == nil {
		t.Error("expected five_hour ResetsAt to be set")
	}
	sevenDay := meterByLabel(snap.Meters, "seven_day")
	if sevenDay == nil || sevenDay.Used == nil || *sevenDay.Used != "120" {
		t.Errorf("seven_day = %v, want used=120", sevenDay)
	}
	if sevenDay == nil || sevenDay.Limit == nil || *sevenDay.Limit != "400" {
		t.Errorf("seven_day = %v, want limit=400", sevenDay)
	}
	if meterByLabel(snap.Meters, "Extra usage") == nil {
		t.Error("expected an Extra usage meter")
	}
}

func TestClaudeAdminAPIStrategyParsesCostReport(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("claude-admin-cost.json"), nil)
	strategy := &claudeAdminAPIStrategy{apiKey: "sk-ant-admin-test", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	if snap.Currency == nil || *snap.Currency != "USD" {
		t.Errorf("Currency = %v, want USD", snap.Currency)
	}
	spend := meterByLabel(snap.Meters, "Spend (7d)")
	if spend == nil || spend.Used == nil || *spend.Used != "12.5" {
		t.Errorf("Spend (7d) = %v, want used=12.5", spend)
	}
	messages := meterByLabel(snap.Meters, "Messages (7d)")
	if messages == nil || messages.Used == nil || *messages.Used != "340" {
		t.Errorf("Messages (7d) = %v, want used=340", messages)
	}
}

func TestClaudeAdminAPIUsesCostReportEndpoint(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("claude-admin-cost.json"), nil)
	strategy := &claudeAdminAPIStrategy{apiKey: "sk-ant-admin-test", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.URL.Path; got != "/v1/organizations/cost_report" {
		t.Errorf("path = %q, want /v1/organizations/cost_report", got)
	}
	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer sk-ant-admin-test" {
		t.Errorf("Authorization = %q, want Bearer sk-ant-admin-test", got)
	}
}

func TestClaudeOAuthUsesBetaHeader(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("claude-oauth-usage.json"), nil)
	strategy := &claudeOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("anthropic-beta"); got != "oauth-2025-04-20" {
		t.Errorf("anthropic-beta = %q, want oauth-2025-04-20", got)
	}
	if got := transport.lastRequest.URL.Path; got != "/api/oauth/usage" {
		t.Errorf("path = %q, want /api/oauth/usage", got)
	}
}

// MARK: Provider configuration

func TestClaudeProviderUnconfiguredWithoutCredentials(t *testing.T) {
	p := &ClaudeProvider{}
	if p.IsConfigured() {
		t.Error("expected unconfigured without any credential")
	}
}

func TestClaudeProviderConfiguredWithAdminKey(t *testing.T) {
	p := &ClaudeProvider{adminKey: "sk-ant-admin-abc"}
	if !p.IsConfigured() {
		t.Error("expected configured with an admin key")
	}
}

func TestClaudeProviderConfiguredWithOAuthToken(t *testing.T) {
	p := &ClaudeProvider{oauthToken: "sk-ant-oat-abc"}
	if !p.IsConfigured() {
		t.Error("expected configured with an OAuth token")
	}
}

func TestClaudeProviderOrdersStrategiesAPIThenOAuth(t *testing.T) {
	p := &ClaudeProvider{adminKey: "sk-ant-admin-abc", oauthToken: "sk-ant-oat-abc"}
	strategies := p.Strategies()
	if len(strategies) != 2 {
		t.Fatalf("Strategies() = %d, want 2", len(strategies))
	}
	if strategies[0].Source() != "api" || strategies[1].Source() != "oauth" {
		t.Errorf("Strategies() sources = [%s, %s], want [api, oauth]", strategies[0].Source(), strategies[1].Source())
	}
}

// MARK: File credential store (read-only)

func TestClaudeFileTokenPrefersDefaultAccount(t *testing.T) {
	home := t.TempDir()
	claudeDir := filepath.Join(home, ".claude")
	if err := os.MkdirAll(claudeDir, 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	store := `{"oauthAccount":{"default":{"accessToken":"sk-ant-oat-default"},"work":{"accessToken":"sk-ant-oat-work"}}}`
	if err := os.WriteFile(filepath.Join(claudeDir, ".credentials.json"), []byte(store), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	t.Setenv("HOME", home)
	if got := readClaudeFileToken(); got != "sk-ant-oat-default" {
		t.Errorf("readClaudeFileToken() = %q, want sk-ant-oat-default", got)
	}
}

func TestClaudeFileTokenMissingFileYieldsEmpty(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	if got := readClaudeFileToken(); got != "" {
		t.Errorf("readClaudeFileToken() = %q, want empty", got)
	}
}

// MARK: Errors never leak token material

func TestClaudeAuthErrorIsUnderstandableWithoutTokenMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &claudeOAuthStrategy{accessToken: "sk-ant-oat-secret-token-material", client: client}

	_, err := strategy.Fetch(context.Background())
	clErr, ok := err.(*claudeError)
	if !ok {
		t.Fatalf("expected *claudeError, got %v (%T)", err, err)
	}
	message := strings.ToLower(clErr.Error())
	if !strings.Contains(message, "re-auth") {
		t.Errorf("error = %q, want it to mention re-auth", clErr.Error())
	}
	if strings.Contains(clErr.Error(), "sk-ant") {
		t.Error("token material must never leak into the error")
	}
}

// MARK: 429 -> RateLimitedError

func TestClaudeOAuthStrategyHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "8"})
	strategy := &claudeOAuthStrategy{accessToken: "sk-ant-oat-test", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "claude" {
		t.Errorf("ProviderID = %q, want claude", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 8 {
		t.Errorf("RetryAfterSeconds = %v, want 8", rateLimited.RetryAfterSeconds)
	}
}

func TestClaudeAdminAPIStrategyHTTP429MapsToRateLimitedWithoutRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &claudeAdminAPIStrategy{apiKey: "sk-ant-admin-test", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "claude" {
		t.Errorf("ProviderID = %q, want claude", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
