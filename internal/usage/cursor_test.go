package usage

import (
	"context"
	"strings"
	"testing"
	"time"
)

func TestCursorParsesUsageSummaryFixture(t *testing.T) {
	client, _ := fakeClient(200, loadFixture("cursor-usage-summary.json"), nil)
	strategy := &cursorWebStrategy{cookieHeader: "WorkosCursorSessionToken=user_abc123%3A%3Atoken", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "cursor" {
		t.Errorf("ProviderID = %q, want cursor", snap.ProviderID)
	}
	if snap.Source != "web" {
		t.Errorf("Source = %q, want web", snap.Source)
	}
	plan := meterByLabel(snap.Meters, "Plan usage")
	if plan == nil || plan.Used == nil || *plan.Used != "95" {
		t.Errorf("Plan usage = %v, want used=95", plan)
	}
	if plan == nil || plan.Unit != "%" {
		t.Errorf("Plan usage unit = %v, want %%", plan)
	}
	if plan == nil || plan.ResetsAt == nil {
		t.Fatal("expected Plan usage ResetsAt to be set")
	}
	want := time.Date(2026, 9, 1, 0, 0, 0, 0, time.UTC)
	if !plan.ResetsAt.Equal(want) {
		t.Errorf("ResetsAt = %v, want %v", plan.ResetsAt, want)
	}
	if auto := meterByLabel(snap.Meters, "Auto usage"); auto == nil || auto.Used == nil || *auto.Used != "85" {
		t.Errorf("Auto usage = %v, want used=85", auto)
	}
	if api := meterByLabel(snap.Meters, "API usage"); api == nil || api.Used == nil || *api.Used != "15" {
		t.Errorf("API usage = %v, want used=15", api)
	}
	onDemand := meterByLabel(snap.Meters, "On-demand usage")
	if onDemand == nil || onDemand.Used == nil || *onDemand.Used != "734" {
		t.Errorf("On-demand usage = %v, want used=734", onDemand)
	}
	if onDemand == nil || onDemand.Limit == nil || *onDemand.Limit != "10000" {
		t.Errorf("On-demand usage = %v, want limit=10000", onDemand)
	}
	if onDemand == nil || onDemand.Unit != "USD" {
		t.Errorf("On-demand usage unit = %v, want USD", onDemand)
	}
}

func TestCursorWebStrategySendsCookieHeader(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("cursor-usage-summary.json"), nil)
	strategy := &cursorWebStrategy{cookieHeader: "WorkosCursorSessionToken=user_abc123%3A%3Atok", client: client}

	if _, err := strategy.Fetch(context.Background()); err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if got := transport.lastRequest.Header.Get("Cookie"); got != "WorkosCursorSessionToken=user_abc123%3A%3Atok" {
		t.Errorf("Cookie = %q, want the session cookie", got)
	}
	if got := transport.lastRequest.URL.Host; got != "cursor.com" {
		t.Errorf("host = %q, want cursor.com", got)
	}
}

func TestCursorWebStrategyRejectsInvalidSessionWithoutLeakingCookie(t *testing.T) {
	client, _ := fakeClient(401, []byte("nope"), nil)
	strategy := &cursorWebStrategy{cookieHeader: "WorkosCursorSessionToken=user_abc123%3A%3Asecret-token", client: client}

	_, err := strategy.Fetch(context.Background())
	curErr, ok := err.(*cursorError)
	if !ok || curErr.kind != "invalid_credentials" {
		t.Fatalf("expected *cursorError{kind: invalid_credentials}, got %v (%T)", err, err)
	}
	if strings.Contains(curErr.Error(), "secret-token") {
		t.Error("session material must never leak into the error")
	}
}

func TestCursorProviderUnconfiguredWithoutCookie(t *testing.T) {
	p := &CursorProvider{cookieHeader: ""}
	if p.IsConfigured() {
		t.Error("expected unconfigured without a cookie")
	}
}

func TestCursorProviderConfiguredWithCookie(t *testing.T) {
	p := &CursorProvider{cookieHeader: "WorkosCursorSessionToken=x"}
	if !p.IsConfigured() {
		t.Error("expected configured with a cookie")
	}
}

// MARK: 429 -> RateLimitedError

func TestCursorWebStrategyHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	client, _ := fakeClient(429, nil, map[string]string{"Retry-After": "30"})
	strategy := &cursorWebStrategy{cookieHeader: "WorkosCursorSessionToken=user_abc123%3A%3Atoken", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "cursor" {
		t.Errorf("ProviderID = %q, want cursor", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 30 {
		t.Errorf("RetryAfterSeconds = %v, want 30", rateLimited.RetryAfterSeconds)
	}
}

func TestCursorWebStrategyHTTP429WithoutRetryAfterHeaderYieldsNilRetryAfter(t *testing.T) {
	client, _ := fakeClient(429, nil, nil)
	strategy := &cursorWebStrategy{cookieHeader: "WorkosCursorSessionToken=user_abc123%3A%3Atoken", client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.RetryAfterSeconds != nil {
		t.Errorf("RetryAfterSeconds = %v, want nil", rateLimited.RetryAfterSeconds)
	}
}
