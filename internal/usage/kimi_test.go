package usage

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestKimiParsesAPIUsageFixture(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("kimi-api-usages.json"), nil)
	strategy := &kimiAPIStrategy{apiKey: "kimi-test-key", baseURL: kimiDefaultAPIBase, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "kimi" {
		t.Errorf("ProviderID = %q, want kimi", snap.ProviderID)
	}
	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	weekly := meterByLabel(snap.Meters, "Weekly quota")
	if weekly == nil || weekly.Used == nil || *weekly.Used != "214" {
		t.Errorf("Weekly quota = %v, want used=214", weekly)
	}
	if weekly == nil || weekly.Limit == nil || *weekly.Limit != "2048" {
		t.Errorf("Weekly quota = %v, want limit=2048", weekly)
	}
	if weekly == nil || weekly.Unit != "requests" {
		t.Errorf("Weekly quota unit = %v, want requests", weekly)
	}
	if weekly == nil || weekly.ResetsAt == nil {
		t.Error("expected Weekly quota ResetsAt to be set")
	}
	window := meterByLabel(snap.Meters, "5h window")
	if window == nil || window.Used == nil || *window.Used != "139" {
		t.Errorf("5h window = %v, want used=139", window)
	}
	if window == nil || window.Limit == nil || *window.Limit != "200" {
		t.Errorf("5h window = %v, want limit=200", window)
	}
	if window == nil || window.Unit != "requests" {
		t.Errorf("5h window unit = %v, want requests", window)
	}
	if window == nil || window.ResetsAt == nil {
		t.Error("expected 5h window ResetsAt to be set")
	}
}

func TestKimiParsesWebUsageFixture(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("kimi-web-usages.json"), nil)
	strategy := &kimiWebStrategy{authToken: "kimi-test-web-token", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "kimi" {
		t.Errorf("ProviderID = %q, want kimi", snap.ProviderID)
	}
	if snap.Source != "web" {
		t.Errorf("Source = %q, want web", snap.Source)
	}
	weekly := meterByLabel(snap.Meters, "Weekly quota")
	if weekly == nil || weekly.Limit == nil || *weekly.Limit != "2048" {
		t.Errorf("Weekly quota = %v, want limit=2048", weekly)
	}
	if meterByLabel(snap.Meters, "5h window") == nil {
		t.Error("expected a 5h window meter")
	}
}

func TestKimiResetTimeParsesNanosecondFractionalISO8601(t *testing.T) {
	got := kimiParseResetTime("2026-01-09T15:23:13.716839300Z")
	if got == nil {
		t.Fatal("expected a parsed time")
	}
	wantSeconds := 1767972193.7168393
	gotSeconds := float64(got.Unix()) + float64(got.Nanosecond())/1e9
	if diff := gotSeconds - wantSeconds; diff > 0.001 || diff < -0.001 {
		t.Errorf("time = %v, want ~%v (diff %v)", gotSeconds, wantSeconds, diff)
	}
}

func TestKimiResetTimeParsesPlainISO8601(t *testing.T) {
	got := kimiParseResetTime("2026-01-09T15:23:13Z")
	if got == nil {
		t.Fatal("expected a parsed time")
	}
	if got.Unix() != 1767972193 {
		t.Errorf("Unix() = %d, want 1767972193", got.Unix())
	}
}

// MARK: API strategy wire format

func TestKimiAPIStrategyUsesBearerAndDefaultHost(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("kimi-api-usages.json"), nil)
	strategy := &kimiAPIStrategy{apiKey: "kimi-test-key", baseURL: kimiDefaultAPIBase, client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer kimi-test-key" {
		t.Errorf("Authorization = %q, want Bearer kimi-test-key", got)
	}
	want := "https://api.kimi.com/coding/v1/usages"
	if got := transport.lastRequest.URL.String(); got != want {
		t.Errorf("url = %q, want %q", got, want)
	}
	if got := transport.lastRequest.Method; got != "GET" {
		t.Errorf("method = %q, want GET", got)
	}
}

func TestKimiAPIStrategyHonorsBaseOverride(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("kimi-api-usages.json"), nil)
	strategy := &kimiAPIStrategy{apiKey: "kimi-test-key", baseURL: "https://usage.test.local", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	want := "https://usage.test.local/coding/v1/usages"
	if got := transport.lastRequest.URL.String(); got != want {
		t.Errorf("url = %q, want %q", got, want)
	}
}

// MARK: CLI credential path (read-only)

func TestKimiCLIStrategySendsDeviceIdentityHeaders(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("kimi-api-usages.json"), nil)
	strategy := &kimiCLIStrategy{
		accessToken:     "kimi-cli-token",
		identityHeaders: kimiCLIIdentityHeaders("device-1234"),
		baseURL:         kimiDefaultAPIBase,
		client:          client,
	}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer kimi-cli-token" {
		t.Errorf("Authorization = %q, want Bearer kimi-cli-token", got)
	}
	if got := transport.lastRequest.Header.Get("X-Msh-Device-Id"); got != "device-1234" {
		t.Errorf("X-Msh-Device-Id = %q, want device-1234", got)
	}
	if got := transport.lastRequest.Header.Get("X-Msh-Platform"); got == "" {
		t.Error("expected X-Msh-Platform to be set")
	}
}

func TestKimiCLICredentialStoreReadsTokenAndDeviceID(t *testing.T) {
	dir := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, "credentials"), 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	credentials := `{"access_token":"kimi-cli-token-123","expires_at":"2026-12-31T00:00:00Z","token_type":"Bearer"}`
	if err := os.WriteFile(filepath.Join(dir, "credentials", "kimi-code.json"), []byte(credentials), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, "device_id"), []byte("device-abc\n"), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	store := kimiCLICredentialStore{home: dir}
	if got := store.readAccessToken(); got != "kimi-cli-token-123" {
		t.Errorf("readAccessToken() = %q, want kimi-cli-token-123", got)
	}
	if got := store.readDeviceID(); got != "device-abc" {
		t.Errorf("readDeviceID() = %q, want device-abc", got)
	}
}

func TestKimiCLICredentialStoreIsReadOnlyNeverCreatesFiles(t *testing.T) {
	dir := t.TempDir()
	store := kimiCLICredentialStore{home: dir}

	if got := store.readAccessToken(); got != "" {
		t.Errorf("readAccessToken() = %q, want empty", got)
	}
	if got := store.readDeviceID(); got != "" {
		t.Errorf("readDeviceID() = %q, want empty", got)
	}
	if _, err := os.Stat(filepath.Join(dir, "device_id")); err == nil {
		t.Error("device_id must not be created by a read")
	}
	if _, err := os.Stat(filepath.Join(dir, "credentials")); err == nil {
		t.Error("credentials dir must not be created by a read")
	}
}

func TestKimiDefaultCLIHomePrefersCurrentLayout(t *testing.T) {
	t.Setenv("KIMI_CODE_HOME", "")
	home := kimiCLIHome()
	if !strings.HasSuffix(home, "/.kimi-code") {
		t.Errorf("kimiCLIHome() = %q, want a /.kimi-code suffix", home)
	}
}

// MARK: Provider configuration

func TestKimiProviderUnconfiguredWithoutAnyCredential(t *testing.T) {
	p := &KimiProvider{}
	if p.IsConfigured() {
		t.Error("expected unconfigured without any credential")
	}
	if len(p.Strategies()) != 0 {
		t.Error("expected no strategies")
	}
}

func TestKimiProviderConfiguredWithAPIKeyOnly(t *testing.T) {
	p := &KimiProvider{apiKey: "kimi-test-key", baseURL: kimiDefaultAPIBase}
	if !p.IsConfigured() {
		t.Error("expected configured")
	}
	strategies := p.Strategies()
	if len(strategies) != 1 || strategies[0].Source() != "api" {
		t.Errorf("Strategies() = %v, want exactly [api]", strategies)
	}
}

func TestKimiProviderOrdersStrategiesAPIThenCLIThenWeb(t *testing.T) {
	p := &KimiProvider{apiKey: "kimi-test-key", cliAccessToken: "kimi-cli-token-123", authToken: "kimi-test-web-token", baseURL: kimiDefaultAPIBase}
	strategies := p.Strategies()
	if len(strategies) != 3 {
		t.Fatalf("Strategies() = %d, want 3", len(strategies))
	}
	got := []string{strategies[0].Source(), strategies[1].Source(), strategies[2].Source()}
	want := []string{"api", "cli", "web"}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("Strategies()[%d].Source() = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestKimiCLIStrategyWorksWithoutDeviceID(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("kimi-api-usages.json"), nil)
	strategy := &kimiCLIStrategy{
		accessToken:     "kimi-cli-token",
		identityHeaders: kimiCLIIdentityHeaders(""),
		baseURL:         kimiDefaultAPIBase,
		client:          client,
	}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}
	if snap.Source != "cli" {
		t.Errorf("Source = %q, want cli", snap.Source)
	}
	if got := transport.lastRequest.Header.Get("X-Msh-Device-Id"); got != "" {
		t.Errorf("X-Msh-Device-Id = %q, want empty", got)
	}
}

// MARK: Errors never leak token material

func TestKimiAuthErrorIsUnderstandableWithoutTokenMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &kimiAPIStrategy{apiKey: "kimi-super-secret-key", baseURL: kimiDefaultAPIBase, client: client}

	_, err := strategy.Fetch(context.Background())
	kErr, ok := err.(*kimiError)
	if !ok {
		t.Fatalf("expected *kimiError, got %v (%T)", err, err)
	}
	message := strings.ToLower(kErr.Error())
	if !strings.Contains(message, "credential") {
		t.Errorf("error = %q, want it to mention credential", kErr.Error())
	}
	if strings.Contains(kErr.Error(), "kimi-super-secret-key") {
		t.Error("token material must never leak into the error")
	}
}

// MARK: 429 -> RateLimitedError

func TestKimiAPIStrategyHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "12"})
	strategy := &kimiAPIStrategy{apiKey: "kimi-test-key", baseURL: kimiDefaultAPIBase, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "kimi" {
		t.Errorf("ProviderID = %q, want kimi", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 12 {
		t.Errorf("RetryAfterSeconds = %v, want 12", rateLimited.RetryAfterSeconds)
	}
}

func TestKimiWebStrategyHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &kimiWebStrategy{authToken: "kimi-test-web-token", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "kimi" {
		t.Errorf("ProviderID = %q, want kimi", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
