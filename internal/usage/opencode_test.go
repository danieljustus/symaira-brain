package usage

import (
	"context"
	"strings"
	"testing"
	"time"
)

var openCodeNow = time.Unix(1_784_289_600, 0).UTC() // 2026-08-05T18:00:00Z

func TestOpenCodeWorkspaceOverrideNormalization(t *testing.T) {
	cases := []struct {
		in   string
		want string
	}{
		{"wrk_abc123", "wrk_abc123"},
		{"https://opencode.ai/workspace/wrk_abc123/billing", "wrk_abc123"},
		{"  https://opencode.ai/workspace/wrk_xyz987  ", "wrk_xyz987"},
		{"text wrk_embedded42 more", "wrk_embedded42"},
		{"", ""},
		{"https://opencode.ai/workspace/", ""},
	}
	for _, tc := range cases {
		if got := openCodeNormalizeWorkspaceID(tc.in); got != tc.want {
			t.Errorf("openCodeNormalizeWorkspaceID(%q) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestOpenCodeWebStrategyUsesWorkspaceOverrideWithoutLookup(t *testing.T) {
	client, transport := fakeClient(200, loadFixture("opencode-subscription-json.txt"), nil)
	strategy := &openCodeWebStrategy{cookieHeader: "session=test-cookie", workspaceOverride: "wrk_abc123", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.Source != "web" {
		t.Errorf("Source = %q, want web", snap.Source)
	}
	if !strings.Contains(transport.lastRequest.URL.RawQuery, "id="+openCodeSubscriptionServerID) {
		t.Errorf("query = %q, want it to contain the subscription server id", transport.lastRequest.URL.RawQuery)
	}
	if got := transport.lastRequest.Header.Get("Cookie"); got != "session=test-cookie" {
		t.Errorf("Cookie = %q, want session=test-cookie", got)
	}
	if got := transport.lastRequest.Header.Get("X-Server-Id"); got != openCodeSubscriptionServerID {
		t.Errorf("X-Server-Id = %q, want %q", got, openCodeSubscriptionServerID)
	}
}

func TestOpenCodeWebParsesJSONSubscription(t *testing.T) {
	text := string(loadFixture("opencode-subscription-json.txt"))
	snap, err := openCodeParseSubscription(text, openCodeNow)
	if err != nil {
		t.Fatalf("openCodeParseSubscription(): %v", err)
	}

	rolling := meterByLabel(snap.Meters, "5h window")
	if rolling == nil || rolling.Used == nil || *rolling.Used != "12.34" {
		t.Errorf("5h window = %v, want used=12.34", rolling)
	}
	if rolling == nil || rolling.Unit != "%" {
		t.Errorf("5h window unit = %v, want %%", rolling)
	}
	if rolling == nil || rolling.ResetsAt == nil {
		t.Fatal("expected 5h window ResetsAt to be set")
	}
	if diff := rolling.ResetsAt.Sub(openCodeNow).Seconds(); diff < 3599 || diff > 3601 {
		t.Errorf("5h window reset offset = %v, want ~3600s", diff)
	}
	weekly := meterByLabel(snap.Meters, "This week")
	if weekly == nil || weekly.Used == nil || *weekly.Used != "45.6" {
		t.Errorf("This week = %v, want used=45.6", weekly)
	}
	if weekly == nil || weekly.ResetsAt == nil {
		t.Fatal("expected This week ResetsAt to be set")
	}
	if diff := weekly.ResetsAt.Sub(openCodeNow).Seconds(); diff < 86399 || diff > 86401 {
		t.Errorf("This week reset offset = %v, want ~86400s", diff)
	}
}

func TestOpenCodeWebParsesJSLiteralSubscriptionViaRegex(t *testing.T) {
	text := string(loadFixture("opencode-subscription-js.txt"))
	snap, err := openCodeParseSubscription(text, openCodeNow)
	if err != nil {
		t.Fatalf("openCodeParseSubscription(): %v", err)
	}

	rolling := meterByLabel(snap.Meters, "5h window")
	if rolling == nil || rolling.Used == nil || *rolling.Used != "12.34" {
		t.Errorf("5h window = %v, want used=12.34", rolling)
	}
	if rolling == nil || rolling.ResetsAt == nil {
		t.Fatal("expected 5h window ResetsAt to be set")
	}
	if diff := rolling.ResetsAt.Sub(openCodeNow).Seconds(); diff < 3599 || diff > 3601 {
		t.Errorf("5h window reset offset = %v, want ~3600s", diff)
	}
	weekly := meterByLabel(snap.Meters, "This week")
	if weekly == nil || weekly.Used == nil || *weekly.Used != "45.6" {
		t.Errorf("This week = %v, want used=45.6", weekly)
	}
}

func TestOpenCodeWebFetchesWorkspaceIDWhenNoOverride(t *testing.T) {
	client, transport := scriptedClient(
		scriptedResponse{status: 200, body: loadFixture("opencode-workspaces.txt")},
		scriptedResponse{status: 200, body: loadFixture("opencode-subscription-json.txt")},
	)
	strategy := &openCodeWebStrategy{cookieHeader: "session=test-cookie", client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.Source != "web" {
		t.Errorf("Source = %q, want web", snap.Source)
	}
	if len(transport.requests) != 2 {
		t.Fatalf("requests = %d, want 2", len(transport.requests))
	}
	if !strings.Contains(transport.requests[0].URL.RawQuery, "id="+openCodeWorkspacesServerID) {
		t.Errorf("first request query = %q, want it to contain the workspaces server id", transport.requests[0].URL.RawQuery)
	}
}

func TestOpenCodeWebRejectsSignedOutPayload(t *testing.T) {
	client, _ := fakeClient(200, []byte(`{"error":"please sign in"}`), nil)
	strategy := &openCodeWebStrategy{cookieHeader: "session=stale", workspaceOverride: "wrk_abc123", client: client}

	_, err := strategy.Fetch(context.Background())
	ocErr, ok := err.(*openCodeError)
	if !ok || ocErr.kind != "invalid_credentials" {
		t.Fatalf("expected *openCodeError{kind: invalid_credentials}, got %v (%T)", err, err)
	}
}

func TestOpenCodeProviderUnconfiguredWithoutCookie(t *testing.T) {
	p := &OpenCodeProvider{}
	if p.IsConfigured() {
		t.Error("expected unconfigured without a cookie or workspace")
	}
}

func TestOpenCodeProviderConfiguredWithCookie(t *testing.T) {
	p := &OpenCodeProvider{cookieHeader: "session=abc"}
	if !p.IsConfigured() {
		t.Error("expected configured with a cookie")
	}
	if len(p.Strategies()) != 1 {
		t.Errorf("Strategies() = %d, want 1", len(p.Strategies()))
	}
}
