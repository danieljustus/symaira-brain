package usage

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode"
)

const grokBillingURL = "https://cli-chat-proxy.grok.com/v1/billing?format=credits"
const grokLoginHint = "Grok auth expired. Run `grok login` again; usage never refreshes or writes credentials."

// GrokProvider reads the shared weekly pool, never the legacy monthly quota.
// shortcut: schema v1 has no plan/PAYG metadata, add an agreed schema before exposing badges.
type GrokProvider struct {
	token  string
	status AuthStatus
	client *http.Client
}

// NewGrokProvider prefers GROK_ACCESS_TOKEN (including secret references), then
// reads $GROK_HOME/auth.json or ~/.grok/auth.json without refresh or writeback.
func NewGrokProvider(client *http.Client) *GrokProvider {
	token, source, err := resolveEnv("GROK_ACCESS_TOKEN")
	status := AuthStatus{Status: "available", Detail: "Grok access token configured", Source: source}
	if err != nil {
		return &GrokProvider{status: AuthStatus{Status: "missing", Detail: "Could not resolve GROK_ACCESS_TOKEN; check the credential reference", Source: source}, client: client}
	}
	if os.Getenv("GROK_ACCESS_TOKEN") == "" {
		home := os.Getenv("GROK_HOME")
		if home == "" {
			userHome, err := os.UserHomeDir()
			if err != nil {
				return &GrokProvider{status: AuthStatus{Status: "missing", Detail: "Cannot locate Grok home; set GROK_HOME"}, client: client}
			}
			home = filepath.Join(userHome, ".grok")
		}
		token, status = readGrokCredential(filepath.Join(home, "auth.json"), time.Now())
	} else {
		token = strings.TrimSpace(token)
		if !grokValidToken(token) {
			token = ""
			status.Status, status.Detail = "missing", "Invalid GROK_ACCESS_TOKEN; supply an access token"
		} else if grokTokenExpired(token, time.Now()) {
			token = ""
			status.Status, status.Detail = "expired", grokLoginHint
		}
	}
	return &GrokProvider{token: token, status: status, client: client}
}

func (p *GrokProvider) ID() string             { return "grok" }
func (p *GrokProvider) DisplayName() string    { return "Grok" }
func (p *GrokProvider) IsConfigured() bool     { return p.token != "" }
func (p *GrokProvider) AuthStatus() AuthStatus { return p.status }
func (p *GrokProvider) Strategies() []Strategy {
	if !p.IsConfigured() {
		return nil
	}
	return []Strategy{&grokStrategy{token: p.token, client: p.client}}
}

func grokValidToken(token string) bool {
	return token != "" && !strings.ContainsFunc(token, unicode.IsControl)
}

// Unverified JWT expiry is only a local hint; the server still authenticates.
func grokTokenExpired(token string, now time.Time) bool {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return false
	}
	data, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return false
	}
	var claims struct {
		Exp *float64 `json:"exp"`
	}
	if json.Unmarshal(data, &claims) != nil || claims.Exp == nil {
		return false
	}
	return *claims.Exp <= float64(now.Unix())
}

func readGrokCredential(path string, now time.Time) (string, AuthStatus) {
	status := AuthStatus{Status: "missing", Detail: "Grok not logged in. Run `grok login`.", Source: "file"}
	data, err := readCredentialFile(path)
	if err != nil {
		return "", status
	}
	var entries map[string]struct {
		Key       string `json:"key"`
		ExpiresAt string `json:"expires_at"`
		Expires   string `json:"expires"`
	}
	if json.Unmarshal(data, &entries) != nil {
		status.Detail = "Grok auth invalid. Run `grok login` again."
		return "", status
	}
	var token string
	expired := false
	for _, entry := range entries {
		candidate := strings.TrimSpace(entry.Key)
		if !grokValidToken(candidate) {
			continue
		}
		// Do not silently choose a different billing account from a map.
		if token != "" && token != candidate {
			status.Detail = "Multiple Grok accounts; select one explicitly with GROK_ACCESS_TOKEN"
			return "", status
		}
		token = candidate
		expiry, err := time.Parse(time.RFC3339Nano, strings.TrimSpace(entry.ExpiresAt))
		if err != nil {
			expiry, err = time.Parse(time.RFC3339Nano, strings.TrimSpace(entry.Expires))
		}
		expired = expired || (err == nil && !expiry.After(now)) || grokTokenExpired(token, now)
	}
	if expired {
		status.Status, status.Detail = "expired", grokLoginHint
		return "", status
	}
	if token != "" {
		status.Status, status.Detail = "available", "Signed in via Grok CLI auth store (read-only)"
	}
	return token, status
}

type grokStrategy struct {
	token  string
	client *http.Client
}

func (s *grokStrategy) Source() string { return "api" }
func (s *grokStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, grokBillingURL, nil)
	if err != nil {
		return nil, errors.New("Grok billing request could not be created")
	}
	req.Header.Set("Authorization", "Bearer "+s.token)
	req.Header.Set("X-XAI-Token-Auth", "xai-grok-cli")
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "symbrain")
	resp, err := doUsageRequest(ctx, s.client, req, false)
	if err != nil {
		return nil, errors.New("Grok billing request failed; check your connection")
	}
	body, err := readUsageBodyAndClose(resp.Body)
	if err != nil {
		return nil, errors.New("Grok billing response could not be read")
	}
	switch resp.StatusCode {
	case 401, 403:
		return nil, errors.New(grokLoginHint)
	case 429:
		return nil, &RateLimitedError{ProviderID: "grok", RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
	case 412:
		var payload struct {
			Error string `json:"error"`
		}
		if json.Unmarshal(body, &payload) == nil && strings.Contains(strings.ToLower(payload.Error), "no personal team") {
			return nil, errors.New("Grok team billing unavailable: this account has no personal weekly quota")
		}
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("Grok billing request failed with HTTP %d", resp.StatusCode)
	}
	return grokSnapshot(body, time.Now().UTC())
}

func grokSnapshot(body []byte, now time.Time) (*UsageSnapshot, error) {
	var root struct {
		Config *struct {
			Percent json.RawMessage `json:"creditUsagePercent"`
			Period  *struct {
				Type  string `json:"type"`
				Start string `json:"start"`
				End   string `json:"end"`
			} `json:"currentPeriod"`
		} `json:"config"`
	}
	invalid := &PayloadError{ProviderID: "grok", Detail: "invalid credits config or period"}
	if json.Unmarshal(body, &root) != nil || root.Config == nil || root.Config.Period == nil {
		return nil, invalid
	}
	period := root.Config.Period
	start, err := time.Parse(time.RFC3339Nano, strings.TrimSpace(period.Start))
	if err != nil {
		return nil, invalid
	}
	end, err := time.Parse(time.RFC3339Nano, strings.TrimSpace(period.End))
	if err != nil || !end.After(start) {
		return nil, invalid
	}
	if strings.TrimSpace(period.Type) != "USAGE_PERIOD_TYPE_WEEKLY" {
		return nil, &PayloadError{ProviderID: "grok", Detail: "weekly quota unavailable for this billing period (legacy monthly quota is not weekly)"}
	}
	var used *string
	if raw := root.Config.Percent; len(raw) > 0 {
		var n float64
		if json.Unmarshal(raw, &n) != nil || string(raw) == "null" {
			var text string
			if json.Unmarshal(raw, &text) != nil {
				return nil, invalid
			}
			var parseErr error
			n, parseErr = strconv.ParseFloat(text, 64)
			if parseErr != nil {
				return nil, invalid
			}
		}
		if math.IsNaN(n) || math.IsInf(n, 0) {
			return nil, invalid
		}
		used = strPtr(formatAmount(math.Max(0, math.Min(100, n))))
	}
	// Absence is unknown, not zero, even if a proto-JSON server may omit defaults.
	return &UsageSnapshot{ProviderID: "grok", Meters: []UsageMeter{{Label: "Weekly limit", Used: used, Limit: strPtr("100"), Unit: "%", ResetsAt: &end}}, FetchedAt: now, Source: "api"}, nil
}
