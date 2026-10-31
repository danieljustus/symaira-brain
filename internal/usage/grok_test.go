package usage

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const grokWeeklyFixture = `{"config":{"creditUsagePercent":23.5,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-07-03T04:01:09.238389+00:00","end":"2026-07-10T04:01:09.238389+00:00"},"onDemandCap":{"val":2500}}}`

func TestGrokParser(t *testing.T) {
	now := time.Date(2026, 7, 4, 0, 0, 0, 0, time.UTC)
	for _, tc := range []struct {
		name, from, to, used string
		fails                bool
	}{
		{name: "weekly", used: "23.5"},
		{name: "zero", from: `23.5`, to: `0`, used: "0"},
		{name: "numeric string", from: `23.5`, to: `"25.5"`, used: "25.5"},
		{name: "missing is unknown", from: `"creditUsagePercent":23.5,`, to: ""},
		{name: "upper clamp", from: `23.5`, to: `125`, used: "100"},
		{name: "lower clamp", from: `23.5`, to: `-3`, used: "0"},
		{name: "monthly", from: `USAGE_PERIOD_TYPE_WEEKLY`, to: `USAGE_PERIOD_TYPE_MONTHLY`, fails: true},
		{name: "null", from: `23.5`, to: `null`, fails: true},
		{name: "bool", from: `23.5`, to: `true`, fails: true},
		{name: "nan", from: `23.5`, to: `"NaN"`, fails: true},
		{name: "infinity", from: `23.5`, to: `"Infinity"`, fails: true},
		{name: "bad percent", from: `23.5`, to: `{}`, fails: true},
		{name: "bad date", from: `2026-07-10T04:01:09.238389+00:00`, to: `invalid`, fails: true},
		{name: "backwards", from: `2026-07-10`, to: `2026-07-01`, fails: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			body := grokWeeklyFixture
			if tc.from != "" {
				body = strings.Replace(body, tc.from, tc.to, 1)
			}
			snapshot, err := grokSnapshot([]byte(body), now)
			if tc.fails {
				if err == nil {
					t.Fatal("accepted malformed/unavailable quota")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if snapshot.ProviderID != "grok" || snapshot.Source != "api" || !snapshot.FetchedAt.Equal(now) || len(snapshot.Meters) != 1 {
				t.Fatalf("bad snapshot: %+v", snapshot)
			}
			meter := snapshot.Meters[0]
			if tc.used == "" {
				if meter.Used != nil {
					t.Fatal("missing usage became zero")
				}
			} else if meter.Used == nil || *meter.Used != tc.used {
				t.Fatalf("bad used: %v", meter.Used)
			}
			if meter.Label != "Weekly limit" || meter.Unit != "%" || meter.Limit == nil || *meter.Limit != "100" || meter.ResetsAt == nil {
				t.Fatalf("bad meter: %+v", meter)
			}
			if snapshot.Balance != nil || snapshot.Currency != nil {
				t.Fatal("PAYG cap became spend")
			}
		})
	}
	for _, body := range []string{`{}`, `[]`, `null`, `{"config":{}}`, `not json`} {
		if _, err := grokSnapshot([]byte(body), now); err == nil {
			t.Fatalf("accepted %s", body)
		}
	}
}

func TestGrokTransport(t *testing.T) {
	t.Setenv("GROK_ACCESS_TOKEN", " fake-access ")
	client, transport := fakeClient(200, []byte(grokWeeklyFixture), nil)
	provider := NewGrokProvider(client)
	report := BuildReport(context.Background(), []Provider{provider})
	if len(report.Providers) != 1 || report.Providers[0].Snapshot == nil || report.Providers[0].Error != "" {
		t.Fatalf("bad report: %+v", report)
	}
	req := transport.lastRequest
	if req == nil || req.Method != "GET" || req.URL.String() != grokBillingURL || req.Body != nil {
		t.Fatalf("bad request: %v", req)
	}
	for key, value := range map[string]string{"Authorization": "Bearer fake-access", "Accept": "application/json", "X-XAI-Token-Auth": "xai-grok-cli", "User-Agent": "symbrain"} {
		if req.Header.Get(key) != value {
			t.Errorf("bad %s", key)
		}
	}
}

func TestGrokHTTPFailures(t *testing.T) {
	for _, tc := range []struct {
		status         int
		body, contains string
	}{
		{401, `secret body`, "grok login"}, {403, `secret body`, "grok login"},
		{412, `{"error":"No Personal Team exists"}`, "no personal weekly quota"},
		{412, `{"error":"other precondition"}`, "HTTP 412"},
		{412, `no personal team`, "HTTP 412"},
		{429, `secret body`, "rate limited"}, {500, `secret body`, "HTTP 500"},
		{302, `secret body`, "HTTP 302"}, {200, `secret body`, "malformed usage"},
	} {
		client, _ := fakeClient(tc.status, []byte(tc.body), map[string]string{"Retry-After": "5"})
		_, err := (&grokStrategy{token: "synthetic", client: client}).Fetch(context.Background())
		if err == nil || !strings.Contains(err.Error(), tc.contains) || strings.Contains(err.Error(), "secret body") {
			t.Errorf("%d: %v", tc.status, err)
		}
	}
	client, _ := fakeErrorClient(errors.New("synthetic-secret-value"))
	_, err := (&grokStrategy{token: "synthetic", client: client}).Fetch(context.Background())
	if err == nil || strings.Contains(err.Error(), "synthetic-secret-value") {
		t.Fatalf("leaked transport failure: %v", err)
	}
	client, _ = fakeClient(200, make([]byte, maxUsageResponseBytes+1), nil)
	if _, err = (&grokStrategy{token: "synthetic", client: client}).Fetch(context.Background()); err == nil {
		t.Fatal("accepted oversized response")
	}
}

func TestGrokCredentials(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("GROK_HOME", filepath.Join(home, "override"))
	t.Setenv("GROK_ACCESS_TOKEN", "")
	if err := os.MkdirAll(os.Getenv("GROK_HOME"), 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(os.Getenv("GROK_HOME"), "auth.json")
	fixture := `{"account::client":{"key":" file-token ","refresh_token":"never-read-or-used","expires_at":"2099-01-01T00:00:00Z"}}`
	if err := os.WriteFile(path, []byte(fixture), 0600); err != nil {
		t.Fatal(err)
	}
	p := NewGrokProvider(&http.Client{})
	if !p.IsConfigured() || p.token != "file-token" || p.AuthStatus().Source != "file" {
		t.Fatalf("bad file credential: %+v", p.AuthStatus())
	}
	t.Setenv("GROK_ACCESS_TOKEN", "env-token")
	if p = NewGrokProvider(nil); p.token != "env-token" || p.AuthStatus().Source != "env" {
		t.Fatal("env did not win")
	}
	original := vaultResolver
	t.Cleanup(func() { vaultResolver = original })
	vaultResolver = func(ref, fallback string) (string, error) {
		if ref != "symvault://test/grok" {
			t.Fatalf("bad reference: %s", ref)
		}
		return "vault-token", nil
	}
	t.Setenv("GROK_ACCESS_TOKEN", "symvault://test/grok")
	if p = NewGrokProvider(nil); p.token != "vault-token" || p.AuthStatus().Source != "vault" {
		t.Fatal("vault not resolved")
	}
	vaultResolver = func(string, string) (string, error) { return "", errors.New("do-not-leak") }
	if p = NewGrokProvider(nil); p.IsConfigured() || strings.Contains(p.AuthStatus().Detail, "do-not-leak") {
		t.Fatal("resolution failure fell back or leaked")
	}
	t.Setenv("GROK_ACCESS_TOKEN", "bad\r\ntoken")
	if NewGrokProvider(nil).IsConfigured() {
		t.Fatal("accepted header injection")
	}
	claims := base64.RawURLEncoding.EncodeToString([]byte(`{"exp":1}`))
	t.Setenv("GROK_ACCESS_TOKEN", "x."+claims+".x")
	if p = NewGrokProvider(nil); p.IsConfigured() || p.AuthStatus().Status != "expired" {
		t.Fatal("expired env accepted")
	}
	if after, err := os.ReadFile(path); err != nil || string(after) != fixture {
		t.Fatal("auth store modified")
	}
	t.Setenv("GROK_ACCESS_TOKEN", "")
	for _, tc := range []struct{ body, status string }{
		{`{"a":{"key":"old","expires":"2000-01-01T00:00:00Z"}}`, "expired"},
		{`{"a":{"key":"x.` + claims + `.x"}}`, "expired"},
		{`{"a":{"key":"a"},"b":{"key":"b"}}`, "missing"},
		{`{"a":{"key":"bad\nvalue"}}`, "missing"},
		{`{"a":{"key":true}}`, "missing"}, {`not json`, "missing"},
	} {
		if err := os.WriteFile(path, []byte(tc.body), 0600); err != nil {
			t.Fatal(err)
		}
		p = NewGrokProvider(nil)
		if p.IsConfigured() || p.AuthStatus().Status != tc.status {
			t.Errorf("%s: %+v", tc.body, p.AuthStatus())
		}
		if after, _ := os.ReadFile(path); string(after) != tc.body {
			t.Fatal("auth store modified")
		}
	}
	t.Setenv("GROK_HOME", "")
	if err := os.MkdirAll(filepath.Join(home, ".grok"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, ".grok", "auth.json"), []byte(fixture), 0600); err != nil {
		t.Fatal(err)
	}
	if NewGrokProvider(nil).token != "file-token" {
		t.Fatal("default home not honored")
	}
}

func TestGrokCredentialFileSafety(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "auth.json")
	if err := os.WriteFile(path, []byte(`{"a":{"key":"safe"}}`), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(dir, "link")
	if err := os.Symlink(path, link); err != nil {
		t.Skip(err)
	}
	for _, p := range []string{link, dir, filepath.Join(dir, "missing")} {
		if token, _ := readGrokCredential(p, time.Now()); token != "" {
			t.Fatal("unsafe file accepted")
		}
	}
	if err := os.WriteFile(path, make([]byte, maxCredentialFileBytes+1), 0600); err != nil {
		t.Fatal(err)
	}
	if token, _ := readGrokCredential(path, time.Now()); token != "" {
		t.Fatal("oversized file accepted")
	}
}

func TestGrokMissingUsageJSON(t *testing.T) {
	s, err := grokSnapshot([]byte(strings.Replace(grokWeeklyFixture, `"creditUsagePercent":23.5,`, "", 1)), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	body, err := json.Marshal(s)
	if err != nil || strings.Contains(string(body), `"used"`) {
		t.Fatal("unknown usage serialized as zero")
	}
}
