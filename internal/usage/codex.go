package usage

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"time"
)

const (
	codexProviderID  = "codex"
	codexDisplayName = "Codex"
)

// CodexProvider — Codex (OpenAI) usage provider.
//
// Primary source: the OAuth token from ~/.codex/auth.json ($CODEX_HOME
// override), queried against the wham usage endpoint. The auth file is
// opened strictly read-only — never written, never refreshed by this
// provider. Mirrors symaira-cockpit's CodexUsageProvider.
type CodexProvider struct {
	accessToken string
	credErr     error
	credSource  string
	homeDir     string
	client      *http.Client
}

// NewCodexProvider reads the OAuth token from $CODEX_HOME/auth.json (or
// ~/.codex/auth.json when CODEX_HOME is unset).
func NewCodexProvider(client *http.Client) *CodexProvider {
	if client == nil {
		client = http.DefaultClient
	}
	home := codexHomeDir()
	accessToken, credSource, credErr := resolveFileCredential("CODEX_ACCESS_TOKEN", func() string {
		return readCodexAccessToken(home)
	})
	return &CodexProvider{
		accessToken: accessToken,
		credErr:     credErr,
		credSource:  credSource,
		homeDir:     home,
		client:      client,
	}
}

func codexHomeDir() string {
	if home := os.Getenv("CODEX_HOME"); home != "" {
		return home
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".codex")
}

// readCodexAccessToken opens <home>/auth.json read-only and returns the
// OAuth access token (prefers the top-level access_token, falls back to
// tokens.access_token), or "" when absent.
func readCodexAccessToken(home string) string {
	data, err := os.ReadFile(filepath.Join(home, "auth.json"))
	if err != nil {
		return ""
	}
	var root map[string]any
	if err := json.Unmarshal(data, &root); err != nil {
		return ""
	}
	if token, ok := root["access_token"].(string); ok && token != "" {
		return token
	}
	if tokens, ok := root["tokens"].(map[string]any); ok {
		if token, ok := tokens["access_token"].(string); ok && token != "" {
			return token
		}
	}
	return ""
}

func (p *CodexProvider) ID() string          { return codexProviderID }
func (p *CodexProvider) DisplayName() string { return codexDisplayName }
func (p *CodexProvider) IsConfigured() bool  { return p.accessToken != "" }

func (p *CodexProvider) Strategies() []Strategy {
	if p.accessToken == "" {
		return nil
	}
	return []Strategy{&codexOAuthStrategy{accessToken: p.accessToken, client: p.client}}
}

func (p *CodexProvider) AuthStatus() AuthStatus {
	if p.accessToken != "" {
		return AuthStatus{Status: "available", Detail: "Signed in via Codex CLI OAuth (CODEX_ACCESS_TOKEN or auth.json)", Source: p.credSource}
	}
	if p.credErr != nil {
		return authErrStatus(p.credErr)
	}
	if _, err := os.Stat(filepath.Join(p.homeDir, "auth.json")); err == nil {
		return AuthStatus{Status: "expired", Detail: "Codex auth file found but no valid token — re-auth with the Codex CLI", Source: "file"}
	}
	return AuthStatus{Status: "missing", Detail: "No Codex credentials found — sign in with the Codex CLI"}
}

// codexOAuthStrategy fetches Codex usage from the wham endpoint:
// GET https://chatgpt.com/backend-api/wham/usage with
// Authorization: Bearer <token>.
type codexOAuthStrategy struct {
	accessToken string
	client      *http.Client
}

func (s *codexOAuthStrategy) Source() string { return "oauth" }

func (s *codexOAuthStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://chatgpt.com/backend-api/wham/usage", nil)
	if err != nil {
		return nil, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.accessToken)

	payload, err := FetchJSON[codexWhamUsage](ctx, req, codexProviderID, s.client)
	if err != nil {
		return nil, err
	}

	var meters []UsageMeter
	// Session (primary) and weekly (secondary) windows as separate meters.
	if payload.RateLimit != nil {
		for _, window := range []*codexWindow{payload.RateLimit.PrimaryWindow, payload.RateLimit.SecondaryWindow} {
			if window == nil || window.Limit == nil || *window.Limit <= 0 {
				continue
			}
			used := 0.0
			if window.Utilized != nil {
				used = *window.Utilized
			}
			meters = append(meters, UsageMeter{
				Label:    window.Window,
				Used:     strPtr(formatAmount(used)),
				Limit:    strPtr(formatAmount(*window.Limit)),
				Unit:     "%",
				ResetsAt: window.ResetDate,
			})
		}
	}
	// Model-specific additional rate limits (e.g. Codex Spark).
	for _, extra := range payload.AdditionalRateLimits {
		if extra.Limit == nil || *extra.Limit <= 0 {
			continue
		}
		label := extra.Window
		if extra.Title != nil {
			label = *extra.Title
		}
		var used *string
		if extra.Utilized != nil {
			used = strPtr(formatAmount(*extra.Utilized))
		}
		meters = append(meters, UsageMeter{
			Label:    label,
			Used:     used,
			Limit:    strPtr(formatAmount(*extra.Limit)),
			Unit:     "%",
			ResetsAt: extra.ResetDate,
		})
	}

	return &UsageSnapshot{
		ProviderID: codexProviderID,
		Meters:     meters,
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

type codexWhamUsage struct {
	RateLimit            *codexRateLimit            `json:"rate_limit"`
	AdditionalRateLimits []codexAdditionalRateLimit `json:"additional_rate_limits"`
}

type codexRateLimit struct {
	PrimaryWindow   *codexWindow `json:"primary_window"`
	SecondaryWindow *codexWindow `json:"secondary_window"`
}

type codexWindow struct {
	Window    string     `json:"window"`
	Utilized  *float64   `json:"utilized"`
	Limit     *float64   `json:"limit"`
	ResetDate *time.Time `json:"reset_date"`
}

type codexAdditionalRateLimit struct {
	Window    string     `json:"window"`
	Title     *string    `json:"title"`
	Utilized  *float64   `json:"utilized"`
	Limit     *float64   `json:"limit"`
	ResetDate *time.Time `json:"reset_date"`
}
