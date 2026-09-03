package usage

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// The shape the endpoint returns today: every quota under
// `quota_snapshots`, with the reset date on the envelope. Parsing only the
// older `copilot.chat` shape is what left the card empty for a signed-in
// Copilot account — the fetch succeeded and produced no meters.
func TestCopilotParsesQuotaSnapshots(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("copilot-user.json"), nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.com", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	// 300 granted, 255 left — the meter reports what has been consumed.
	premium := meterByLabel(snap.Meters, "Premium requests")
	if premium == nil || premium.Used == nil || *premium.Used != "45" {
		t.Fatalf("Premium requests = %v, want used=45", premium)
	}
	if premium.Limit == nil || *premium.Limit != "300" {
		t.Errorf("Premium requests limit = %v, want 300", premium.Limit)
	}
	if premium.Unit != "requests" {
		t.Errorf("Premium requests unit = %q, want requests", premium.Unit)
	}
	if premium.ResetsAt == nil || premium.ResetsAt.Format("2006-01-02") != "2026-10-01" {
		t.Errorf("Premium requests ResetsAt = %v, want the envelope reset date", premium.ResetsAt)
	}
	// Chat and completions are unlimited on this plan: a bar that can never
	// move is worse than no bar.
	if len(snap.Meters) != 1 {
		t.Errorf("len(Meters) = %d, want 1 (unlimited quotas skipped)", len(snap.Meters))
	}
}

// A quota that reports only a remaining percentage still meters, and a
// quota id GitHub adds later is labelled readably rather than skipped.
func TestCopilotMetersPercentOnlyAndUnknownQuotas(t *testing.T) {
	percent := 40.0
	response := copilotUserResponse{
		QuotaResetDate: "2026-10-01",
		QuotaSnapshots: map[string]copilotQuotaSnapshot{
			"agent_sessions": {QuotaID: "agent_sessions", PercentRemaining: &percent, HasQuota: true},
		},
	}

	meters := response.meters()
	if len(meters) != 1 {
		t.Fatalf("len(meters) = %d, want 1", len(meters))
	}
	if meters[0].Label != "Agent sessions" {
		t.Errorf("Label = %q, want %q", meters[0].Label, "Agent sessions")
	}
	if meters[0].Unit != "%" || meters[0].Used == nil || *meters[0].Used != "60" {
		t.Errorf("meter = %+v, want 60%% used", meters[0])
	}
	if meters[0].ResetsAt == nil {
		t.Error("expected the plain reset date to be parsed")
	}
}

func TestCopilotSkipsQuotasItCannotMeter(t *testing.T) {
	entitlement := 300.0
	response := copilotUserResponse{QuotaSnapshots: map[string]copilotQuotaSnapshot{
		"chat":                 {Entitlement: &entitlement, Unlimited: true, HasQuota: true},
		"completions":          {Entitlement: &entitlement, HasQuota: false},
		"premium_interactions": {HasQuota: true},
	}}
	if meters := response.meters(); len(meters) != 0 {
		t.Errorf("meters = %+v, want none", meters)
	}
}

// The previous shape is still accepted: the endpoint is undocumented, and
// the parser should not break again if it moves back.
func TestCopilotParsesLegacyUserResponse(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("copilot-user-legacy.json"), nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.com", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "copilot" {
		t.Errorf("ProviderID = %q, want copilot", snap.ProviderID)
	}
	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	premium := meterByLabel(snap.Meters, "Premium requests")
	if premium == nil || premium.Used == nil || *premium.Used != "45" {
		t.Errorf("Premium requests = %v, want used=45", premium)
	}
	if premium == nil || premium.Limit == nil || *premium.Limit != "300" {
		t.Errorf("Premium requests = %v, want limit=300", premium)
	}
	if premium == nil || premium.Unit != "requests" {
		t.Errorf("Premium requests unit = %v, want requests", premium)
	}
	if premium == nil || premium.ResetsAt == nil {
		t.Error("expected Premium requests ResetsAt to be set")
	}
	if skills := meterByLabel(snap.Meters, "Skills chat requests"); skills == nil || skills.Used == nil || *skills.Used != "12" {
		t.Errorf("Skills chat requests = %v, want used=12", skills)
	}
}

func TestCopilotProviderUnconfiguredWithoutToken(t *testing.T) {
	p := &CopilotProvider{accessToken: ""}
	if p.IsConfigured() {
		t.Error("expected unconfigured without a token")
	}
}

func TestCopilotProviderConfiguredWithToken(t *testing.T) {
	p := &CopilotProvider{accessToken: "ghu_test"}
	if !p.IsConfigured() {
		t.Error("expected configured with a token")
	}
}

// MARK: Token store (read-only local config)

func TestCopilotTokenStoreReadsAppsJSON(t *testing.T) {
	dir := t.TempDir()
	store := `{"github.com:Iv1.test": {"user": "dev", "oauth_token": "ghu_synthetic-token"}}`
	path := filepath.Join(dir, "apps.json")
	if err := os.WriteFile(path, []byte(store), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	before, _ := os.ReadFile(path)
	token := readCopilotToken(dir)
	after, _ := os.ReadFile(path)

	if token != "ghu_synthetic-token" {
		t.Errorf("token = %q, want ghu_synthetic-token", token)
	}
	if string(before) != string(after) {
		t.Error("config must stay byte-identical after a read")
	}
}

func TestCopilotTokenStoreMissingDirYieldsEmpty(t *testing.T) {
	if got := readCopilotToken(filepath.Join(t.TempDir(), "does-not-exist")); got != "" {
		t.Errorf("token = %q, want empty", got)
	}
}

// MARK: Errors never leak token material

func TestCopilotAuthErrorIsUnderstandableWithoutTokenMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_secret-token-material", host: "github.com", client: client}

	_, err := strategy.Fetch(context.Background())
	copErr, ok := err.(*copilotError)
	if !ok {
		t.Fatalf("expected *copilotError, got %v (%T)", err, err)
	}
	message := strings.ToLower(copErr.Error())
	if !strings.Contains(message, "re-auth") {
		t.Errorf("error = %q, want it to mention re-auth", copErr.Error())
	}
	if strings.Contains(copErr.Error(), "ghu_") {
		t.Error("token material must never leak into the error")
	}
}

func TestCopilotAuthorizationHeaderUsesBearerToken(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("copilot-user.json"), nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.com", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer ghu_test" {
		t.Errorf("Authorization = %q, want Bearer ghu_test", got)
	}
	if got := transport.lastRequest.URL.Host; got != "api.github.com" {
		t.Errorf("host = %q, want api.github.com", got)
	}
}

func TestCopilotEnterpriseHostUsesAPIV3Base(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("copilot-user.json"), nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.example.com", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	want := "https://github.example.com/api/v3/copilot_internal/user"
	if got := transport.lastRequest.URL.String(); got != want {
		t.Errorf("url = %q, want %q", got, want)
	}
}

// MARK: 429 -> RateLimitedError

func TestCopilotHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "60"})
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.com", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "copilot" {
		t.Errorf("ProviderID = %q, want copilot", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 60 {
		t.Errorf("RetryAfterSeconds = %v, want 60", rateLimited.RetryAfterSeconds)
	}
}

func TestCopilotHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &copilotAPIStrategy{accessToken: "ghu_test", host: "github.com", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
