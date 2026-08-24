package usage

import (
	"context"
	"strings"
	"testing"
)

func meterByLabel(meters []UsageMeter, label string) *UsageMeter {
	for i := range meters {
		if meters[i].Label == label {
			return &meters[i]
		}
	}
	return nil
}

func TestOpenRouterParsesCreditsResponse(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("openrouter-credits.json"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "openrouter" {
		t.Errorf("ProviderID = %q, want openrouter", snap.ProviderID)
	}
	if snap.Source != "api" {
		t.Errorf("Source = %q, want api", snap.Source)
	}
	if snap.Currency == nil || *snap.Currency != "USD" {
		t.Errorf("Currency = %v, want USD", snap.Currency)
	}
	if snap.Balance == nil || *snap.Balance != "42.75" {
		t.Errorf("Balance = %v, want 42.75", snap.Balance)
	}
	if meterByLabel(snap.Meters, "Key limit") == nil {
		t.Error("expected a Key limit meter")
	}
	if meterByLabel(snap.Meters, "Requests") == nil {
		t.Error("expected a Requests meter")
	}
}

func TestOpenRouterParsesResponseWithoutLimit(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("openrouter-no-limit.json"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.Balance != nil {
		t.Errorf("Balance = %v, want nil", snap.Balance)
	}
	if meterByLabel(snap.Meters, "Spend") == nil {
		t.Error("expected a Spend meter")
	}
	if meterByLabel(snap.Meters, "Key limit") != nil {
		t.Error("expected no Key limit meter")
	}
}

func TestOpenRouterOmitsRequestsMeterWhenRateLimitIsNotReal(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("openrouter-no-request-limit.json"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}
	if meterByLabel(snap.Meters, "Requests") != nil {
		t.Error("expected no Requests meter when rate_limit.requests is -1")
	}
}

func TestOpenRouterKeepsRequestsMeterWhenRateLimitIsReal(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("openrouter-credits.json"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}
	meter := meterByLabel(snap.Meters, "Requests")
	if meter == nil {
		t.Fatal("expected a Requests meter")
	}
	if meter.Limit == nil || *meter.Limit != "500" {
		t.Errorf("Requests limit = %v, want 500", meter.Limit)
	}
}

func TestOpenRouterProviderUnconfiguredWithoutKey(t *testing.T) {
	t.Setenv("OPENROUTER_API_KEY", "")
	p := NewOpenRouterProvider(nil)
	if p.IsConfigured() {
		t.Error("expected unconfigured without OPENROUTER_API_KEY")
	}
	if len(p.Strategies()) != 0 {
		t.Error("expected no strategies when unconfigured")
	}
}

func TestOpenRouterProviderConfiguredWithKey(t *testing.T) {
	t.Setenv("OPENROUTER_API_KEY", "sk-or-v1-abc")
	p := NewOpenRouterProvider(nil)
	if !p.IsConfigured() {
		t.Error("expected configured with OPENROUTER_API_KEY set")
	}
	if len(p.Strategies()) != 1 {
		t.Errorf("Strategies() = %d, want 1", len(p.Strategies()))
	}
}

// MARK: Errors never leak key material

func TestOpenRouterAuthErrorIsUnderstandableWithoutKeyMaterial(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-secret-key-material", baseURL: openRouterDefaultBase, client: client}

	_, err := strategy.Fetch(context.Background())
	var notConfigured *NotConfiguredError
	if !asNotConfigured(err, &notConfigured) {
		t.Fatalf("expected *NotConfiguredError, got %v (%T)", err, err)
	}
	if notConfigured.ProviderID != "openrouter" {
		t.Errorf("ProviderID = %q, want openrouter", notConfigured.ProviderID)
	}
	if strings.Contains(err.Error(), "sk-or-v1") {
		t.Error("key material must never leak into the error")
	}
}

func TestOpenRouterNetworkErrorWrapsWithoutLeakingKey(t *testing.T) {
	client, _ := fakeErrorClient(errBoom)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-secret-key-material", baseURL: openRouterDefaultBase, client: client}

	_, err := strategy.Fetch(context.Background())
	httpErr, ok := err.(*HTTPError)
	if !ok || httpErr.Kind != "network" {
		t.Fatalf("expected *HTTPError{Kind: network}, got %v (%T)", err, err)
	}
	if !strings.Contains(err.Error(), "request failed") {
		t.Errorf("error = %q, want it to mention 'request failed'", err.Error())
	}
}

func TestOpenRouterAuthorizationHeaderUsesBearerToken(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("openrouter-credits.json"), nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test-key", baseURL: openRouterDefaultBase, client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Authorization"); got != "Bearer sk-or-v1-test-key" {
		t.Errorf("Authorization = %q, want Bearer sk-or-v1-test-key", got)
	}
	if got := transport.lastRequest.Header.Get("X-Title"); got != "symbrain" {
		t.Errorf("X-Title = %q, want symbrain", got)
	}
}

// MARK: 429 -> RateLimitedError

func TestOpenRouterHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "42"})
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "openrouter" {
		t.Errorf("ProviderID = %q, want openrouter", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 42 {
		t.Errorf("RetryAfterSeconds = %v, want 42", rateLimited.RetryAfterSeconds)
	}
}

func TestOpenRouterHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}

func TestOpenRouterHTTP429WithUnparseableRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "Wed, 21 Oct 2026 07:28:00 GMT"})
	strategy := &openRouterAPIStrategy{apiKey: "sk-or-v1-test", baseURL: openRouterDefaultBase, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil for an HTTP-date value", rateLimited.RetryAfterSeconds)
	}
}

func asNotConfigured(err error, out **NotConfiguredError) bool {
	v, ok := err.(*NotConfiguredError)
	if ok {
		*out = v
	}
	return ok
}

func asRateLimited(err error, out **RateLimitedError) bool {
	v, ok := err.(*RateLimitedError)
	if ok {
		*out = v
	}
	return ok
}
