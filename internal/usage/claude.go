package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"time"
)

const (
	claudeProviderID  = "claude"
	claudeDisplayName = "Claude"
)

// ClaudeProvider — Claude usage provider.
//
// Fallback chain:
//  1. Admin API key (api) — ANTHROPIC_ADMIN_KEY env (symvault:// URIs
//     accepted); queries the
//     organization cost/usage report.
//  2. OAuth (oauth) — ANTHROPIC_OAUTH_TOKEN env (symvault:// URIs
//     accepted), then ~/.claude/.credentials.json, then the macOS login
//     keychain; queries GET /api/oauth/usage (session + weekly windows).
//
// The keychain is where Claude Code actually stores its credentials on
// macOS — the credentials file is what other platforms get — so a Mac with
// a signed-in Claude Code but no file was reported as "not configured",
// the state this provider was least able to explain. It is read through a
// build-tagged helper (claude_keychain_darwin.go, a no-op elsewhere) so the
// provider itself stays portable, mirroring the split corekit's secretref
// package already uses. The file is still tried first: it never raises the
// keychain approval panel.
// Mirrors symaira-cockpit's ClaudeUsageProvider.
type ClaudeProvider struct {
	adminKey       string
	adminErr       error
	adminSource    string
	oauthToken     string
	oauthErr       error
	oauthSource    string
	oauthExpiresAt *time.Time
	client         *http.Client
}

// NewClaudeProvider reads ANTHROPIC_ADMIN_KEY and the Claude CLI's own
// credential file (~/.claude/.credentials.json) from the environment.
func NewClaudeProvider(client *http.Client) *ClaudeProvider {
	if client == nil {
		client = http.DefaultClient
	}
	adminKey, adminSource, adminErr := resolveEnv("ANTHROPIC_ADMIN_KEY")
	oauthToken, oauthSource, oauthErr := resolveFileCredential("ANTHROPIC_OAUTH_TOKEN", readClaudeFileToken)
	var oauthExpiresAt *time.Time
	// Only reached when neither the env var nor the file produced a token:
	// the keychain read can raise an approval panel, so it is the last
	// source tried, never a redundant one.
	if oauthToken == "" && oauthErr == nil {
		if token, expiresAt := readClaudeKeychainCredential(); token != "" {
			oauthToken, oauthSource, oauthExpiresAt = token, "keychain", expiresAt
		}
	}
	return &ClaudeProvider{
		adminKey:       adminKey,
		adminErr:       adminErr,
		adminSource:    adminSource,
		oauthToken:     oauthToken,
		oauthErr:       oauthErr,
		oauthSource:    oauthSource,
		oauthExpiresAt: oauthExpiresAt,
		client:         client,
	}
}

func claudeCredentialsPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".claude", ".credentials.json")
}

// readClaudeFileToken reads ~/.claude/.credentials.json (oauthAccount
// tokens), preferring the "default" account, then any account with an
// access token.
func readClaudeFileToken() string {
	data, err := os.ReadFile(claudeCredentialsPath())
	if err != nil {
		return ""
	}
	var root struct {
		OAuthAccount map[string]struct {
			AccessToken string `json:"accessToken"`
		} `json:"oauthAccount"`
	}
	if err := json.Unmarshal(data, &root); err != nil {
		return ""
	}
	if def, ok := root.OAuthAccount["default"]; ok && def.AccessToken != "" {
		return def.AccessToken
	}
	for _, account := range root.OAuthAccount {
		if account.AccessToken != "" {
			return account.AccessToken
		}
	}
	return ""
}

func (p *ClaudeProvider) ID() string          { return claudeProviderID }
func (p *ClaudeProvider) DisplayName() string { return claudeDisplayName }
func (p *ClaudeProvider) IsConfigured() bool  { return p.adminKey != "" || p.oauthToken != "" }

func (p *ClaudeProvider) Strategies() []Strategy {
	var strategies []Strategy
	if p.adminKey != "" {
		strategies = append(strategies, &claudeAdminAPIStrategy{apiKey: p.adminKey, client: p.client})
	}
	if p.oauthToken != "" {
		strategies = append(strategies, &claudeOAuthStrategy{accessToken: p.oauthToken, client: p.client})
	}
	return strategies
}

func (p *ClaudeProvider) AuthStatus() AuthStatus {
	if p.adminKey == "" && p.oauthToken == "" {
		if p.adminErr != nil {
			return authErrStatus(p.adminErr)
		}
		if p.oauthErr != nil {
			return authErrStatus(p.oauthErr)
		}
		return AuthStatus{Status: "missing", Detail: "No Claude credentials found — add an admin key or sign in with the Claude CLI"}
	}
	if p.oauthToken != "" {
		// A token past its own declared expiry is still handed to the
		// strategy chain — the endpoint is the authority on whether it
		// works — but the status says what the resulting 401 would
		// otherwise leave the user guessing about.
		if p.oauthExpiresAt != nil && p.oauthExpiresAt.Before(time.Now()) {
			return AuthStatus{
				Status: "expired",
				Detail: "Claude Code OAuth token expired — sign in again with the Claude CLI",
				Source: p.oauthSource,
			}
		}
		return AuthStatus{Status: "available", Detail: "Signed in via Claude Code OAuth token", Source: p.oauthSource}
	}
	return AuthStatus{Status: "available", Detail: "Admin API key from ANTHROPIC_ADMIN_KEY", Source: p.adminSource}
}

type claudeError struct {
	kind   string // network | invalid_response | status | unparseable
	status int
	detail string
}

func (e *claudeError) Error() string {
	switch e.kind {
	case "network":
		return fmt.Sprintf("Claude request failed: %s", e.detail)
	case "invalid_response":
		return "Claude returned an invalid response"
	case "status":
		if e.status == 401 || e.status == 403 {
			return fmt.Sprintf("Claude rejected the login (HTTP %d). Re-auth or switch the usage source.", e.status)
		}
		return fmt.Sprintf("Claude request failed with HTTP %d", e.status)
	default:
		return "Claude returned an unreadable response"
	}
}

// claudeAdminAPIStrategy fetches organization-level spend/usage via the
// Anthropic Admin API: GET /v1/organizations/cost_report?bucket_width=1d&limit=7.
type claudeAdminAPIStrategy struct {
	apiKey string
	client *http.Client
}

func (s *claudeAdminAPIStrategy) Source() string { return "api" }

func (s *claudeAdminAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	endpoint := "https://api.anthropic.com/v1/organizations/cost_report?" + url.Values{
		"bucket_width": {"1d"},
		"limit":        {"7"},
	}.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, &claudeError{kind: "network", detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.apiKey)

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, &claudeError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: claudeProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &claudeError{kind: "status", status: resp.StatusCode}
	}

	var payload claudeCostReport
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return nil, &claudeError{kind: "unparseable"}
	}

	var meters []UsageMeter
	if payload.TotalCostUSD != nil {
		meters = append(meters, UsageMeter{Label: "Spend (7d)", Used: strPtr(formatAmount(*payload.TotalCostUSD)), Unit: "USD"})
	}
	if payload.TotalMessages != nil {
		meters = append(meters, UsageMeter{Label: "Messages (7d)", Used: strPtr(formatAmount(float64(*payload.TotalMessages))), Unit: "requests"})
	}

	return &UsageSnapshot{
		ProviderID: claudeProviderID,
		Meters:     meters,
		Currency:   strPtr("USD"),
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

type claudeCostReport struct {
	TotalCostUSD  *float64 `json:"total_cost_usd"`
	TotalMessages *int     `json:"total_messages"`
	TotalTokens   *int     `json:"total_tokens"`
}

// claudeOAuthStrategy fetches session + weekly quota via the Claude OAuth
// usage API: GET /api/oauth/usage.
type claudeOAuthStrategy struct {
	accessToken string
	client      *http.Client
}

func (s *claudeOAuthStrategy) Source() string { return "oauth" }

func (s *claudeOAuthStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://api.anthropic.com/api/oauth/usage", nil)
	if err != nil {
		return nil, &claudeError{kind: "network", detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.accessToken)
	req.Header.Set("anthropic-beta", "oauth-2025-04-20")

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, &claudeError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: claudeProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &claudeError{kind: "status", status: resp.StatusCode}
	}

	var payload claudeOAuthUsage
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return nil, &claudeError{kind: "unparseable"}
	}

	var meters []UsageMeter
	// Session window (5h) and weekly window (7d) as separate meters.
	for _, window := range []struct {
		label string
		w     *claudeUsageWindow
	}{{"five_hour", payload.FiveHour}, {"seven_day", payload.SevenDay}} {
		if window.w == nil || window.w.Limit == nil || *window.w.Limit <= 0 {
			continue
		}
		used := 0.0
		if window.w.Utilized != nil {
			used = *window.w.Utilized
		}
		meters = append(meters, UsageMeter{
			Label:    window.label,
			Used:     strPtr(formatAmount(used)),
			Limit:    strPtr(formatAmount(*window.w.Limit)),
			Unit:     "%",
			ResetsAt: window.w.ResetsAt,
		})
	}
	if extra := payload.ExtraUsage; extra != nil {
		var used, limit *string
		if extra.Used != nil {
			used = strPtr(formatAmount(*extra.Used))
		}
		if extra.Limit != nil {
			limit = strPtr(formatAmount(*extra.Limit))
		}
		meters = append(meters, UsageMeter{Label: "Extra usage", Used: used, Limit: limit, Unit: "USD"})
	}

	return &UsageSnapshot{
		ProviderID: claudeProviderID,
		Meters:     meters,
		Currency:   strPtr("USD"),
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

type claudeOAuthUsage struct {
	FiveHour   *claudeUsageWindow `json:"five_hour"`
	SevenDay   *claudeUsageWindow `json:"seven_day"`
	ExtraUsage *claudeExtraUsage  `json:"extra_usage"`
}

type claudeUsageWindow struct {
	Utilized *float64   `json:"utilized"`
	Limit    *float64   `json:"limit"`
	ResetsAt *time.Time `json:"resets_at"`
}

type claudeExtraUsage struct {
	Used  *float64 `json:"used"`
	Limit *float64 `json:"limit"`
}
