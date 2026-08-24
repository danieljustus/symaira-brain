package usage

import (
	"context"
	"strings"
	"testing"
)

func TestMoonshotParsesInternationalBalance(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("moonshot-balance-ai.json"), nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-test", region: MoonshotRegionInternational, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "moonshot" {
		t.Errorf("ProviderID = %q, want moonshot", snap.ProviderID)
	}
	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	if snap.Currency == nil || *snap.Currency != "USD" {
		t.Errorf("Currency = %v, want USD", snap.Currency)
	}
	if snap.Balance == nil || *snap.Balance != "42.5" {
		t.Errorf("Balance = %v, want 42.5", snap.Balance)
	}
	cash := meterByLabel(snap.Meters, "Cash balance")
	if cash == nil {
		t.Fatal("expected a Cash balance meter")
	}
	if cash.Unit != "USD" {
		t.Errorf("Cash balance unit = %q, want USD — must not leak CNY across the currency boundary", cash.Unit)
	}
	if meterByLabel(snap.Meters, "Voucher balance") == nil {
		t.Error("expected a Voucher balance meter")
	}
}

func TestMoonshotParsesChinaBalanceWithCNY(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("moonshot-balance-cn.json"), nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-cn-test", region: MoonshotRegionChina, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.Currency == nil || *snap.Currency != "CNY" {
		t.Errorf("Currency = %v, want CNY", snap.Currency)
	}
	if snap.Balance == nil || *snap.Balance != "88.88" {
		t.Errorf("Balance = %v, want 88.88", snap.Balance)
	}
	cash := meterByLabel(snap.Meters, "Cash balance")
	if cash == nil || cash.Unit != "CNY" {
		t.Errorf("Cash balance unit = %v, want CNY", cash)
	}
	if meterByLabel(snap.Meters, "Voucher balance") != nil {
		t.Error("expected no Voucher balance meter when voucher_balance is null")
	}
}

func TestMoonshotRegionDefaultsToInternational(t *testing.T) {
	t.Setenv("MOONSHOT_API_KEY", "sk-test")
	t.Setenv("MOONSHOT_REGION", "")
	p := NewMoonshotProvider(nil)
	if len(p.Strategies()) != 1 {
		t.Errorf("Strategies() = %d, want 1", len(p.Strategies()))
	}
}

func TestMoonshotProviderUnconfiguredWithoutKey(t *testing.T) {
	t.Setenv("MOONSHOT_API_KEY", "")
	p := NewMoonshotProvider(nil)
	if p.IsConfigured() {
		t.Error("expected unconfigured without MOONSHOT_API_KEY")
	}
}

func TestMoonshotProviderConfiguredWithKey(t *testing.T) {
	t.Setenv("MOONSHOT_API_KEY", "sk-moonshot-abc")
	p := NewMoonshotProvider(nil)
	if !p.IsConfigured() {
		t.Error("expected configured with MOONSHOT_API_KEY set")
	}
}

// MARK: Errors never leak key material

func TestMoonshotAuthErrorIsUnderstandableWithoutKeyMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-secret-key-material", region: MoonshotRegionInternational, client: client}

	_, err := strategy.Fetch(context.Background())
	var notConfigured *NotConfiguredError
	if !asNotConfigured(err, &notConfigured) {
		t.Fatalf("expected *NotConfiguredError, got %v (%T)", err, err)
	}
	if notConfigured.ProviderID != "moonshot" {
		t.Errorf("ProviderID = %q, want moonshot", notConfigured.ProviderID)
	}
	if strings.Contains(err.Error(), "sk-moonshot") {
		t.Error("key material must never leak into the error")
	}
}

func TestMoonshotAuthorizationHeaderUsesBearerToken(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("moonshot-balance-ai.json"), nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-test-key", region: MoonshotRegionInternational, client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer sk-moonshot-test-key" {
		t.Errorf("Authorization = %q, want Bearer sk-moonshot-test-key", got)
	}
	if got := transport.lastRequest.URL.Host; got != "api.moonshot.ai" {
		t.Errorf("host = %q, want api.moonshot.ai", got)
	}
}

func TestMoonshotChinaRegionUsesCNHost(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("moonshot-balance-cn.json"), nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-test", region: MoonshotRegionChina, client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.URL.Host; got != "api.moonshot.cn" {
		t.Errorf("host = %q, want api.moonshot.cn", got)
	}
}

// MARK: 429 -> RateLimitedError

func TestMoonshotHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "5"})
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-test", region: MoonshotRegionInternational, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "moonshot" {
		t.Errorf("ProviderID = %q, want moonshot", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 5 {
		t.Errorf("RetryAfterSeconds = %v, want 5", rateLimited.RetryAfterSeconds)
	}
}

func TestMoonshotHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &moonshotAPIStrategy{apiKey: "sk-moonshot-test", region: MoonshotRegionInternational, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
