package usage

import (
	"context"
	"net/http"
	"os"
	"time"
)

const (
	openRouterProviderID  = "openrouter"
	openRouterDisplayName = "OpenRouter"
	openRouterDefaultBase = "https://openrouter.ai/api/v1"
)

// OpenRouterProvider — the first vertical AI-usage provider ported to Go.
//
// Data source: the credits/key API at https://openrouter.ai/api/v1.
// Credential resolution for this pass is OPENROUTER_API_KEY only —
// symvault-backed resolution needs a config.toml schema decision that is
// out of scope here (issue #290 progress notes this as a follow-up).
// Mirrors symaira-cockpit's OpenRouterUsageProvider
// (tune/Sources/SymTuneCore/OpenRouterUsageProvider.swift).
type OpenRouterProvider struct {
	apiKey  string
	baseURL string
	client  *http.Client
}

// NewOpenRouterProvider reads OPENROUTER_API_KEY and the OPENROUTER_API_URL
// base-URL override from the environment.
func NewOpenRouterProvider(client *http.Client) *OpenRouterProvider {
	if client == nil {
		client = http.DefaultClient
	}
	baseURL := os.Getenv("OPENROUTER_API_URL")
	if baseURL == "" {
		baseURL = openRouterDefaultBase
	}
	return &OpenRouterProvider{
		apiKey:  os.Getenv("OPENROUTER_API_KEY"),
		baseURL: baseURL,
		client:  client,
	}
}

func (p *OpenRouterProvider) ID() string          { return openRouterProviderID }
func (p *OpenRouterProvider) DisplayName() string { return openRouterDisplayName }
func (p *OpenRouterProvider) IsConfigured() bool  { return p.apiKey != "" }

func (p *OpenRouterProvider) Strategies() []Strategy {
	if p.apiKey == "" {
		return nil
	}
	return []Strategy{&openRouterAPIStrategy{apiKey: p.apiKey, baseURL: p.baseURL, client: p.client}}
}

func (p *OpenRouterProvider) AuthStatus() AuthStatus {
	if p.apiKey == "" {
		return AuthStatus{Status: "missing", Detail: "no API key configured (OPENROUTER_API_KEY)"}
	}
	return AuthStatus{Status: "available", Detail: "API key from OPENROUTER_API_KEY", Source: "env"}
}

// openRouterAPIStrategy fetches OpenRouter credit/usage state via the key
// endpoint: GET {base}/auth/key with Authorization: Bearer <apiKey> returns
// the key's label, total usage, limit, rate limits and free-tier flag.
type openRouterAPIStrategy struct {
	apiKey  string
	baseURL string
	client  *http.Client
}

func (s *openRouterAPIStrategy) Source() string { return "api" }

func (s *openRouterAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.baseURL+"/auth/key", nil)
	if err != nil {
		return nil, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.apiKey)
	req.Header.Set("X-Title", "symbrain")

	payload, err := FetchJSON[openRouterKeyResponse](ctx, req, openRouterProviderID, s.client)
	if err != nil {
		return nil, err
	}

	key := payload.Data
	var meters []UsageMeter

	// Primary meter: spend against the key limit (USD). Mirrors the Swift
	// original's if/else-if exactly (a key either has a limit or doesn't;
	// never both meters).
	switch {
	case key.Limit != nil:
		var used *string
		if key.Usage != nil {
			used = strPtr(formatAmount(*key.Usage))
		}
		meters = append(meters, UsageMeter{
			Label:    "Key limit",
			Used:     used,
			Limit:    strPtr(formatAmount(*key.Limit)),
			Unit:     "USD",
			ResetsAt: key.UsagePeriod.endTime(),
		})
	case key.Usage != nil:
		meters = append(meters, UsageMeter{
			Label:    "Spend",
			Used:     strPtr(formatAmount(*key.Usage)),
			Unit:     "USD",
			ResetsAt: key.UsagePeriod.endTime(),
		})
	}

	// Secondary meter: free-tier daily request rate limit. OpenRouter
	// reports -1 (or other negative sentinels) when no real request cap is
	// configured; that carries no information, so the meter is omitted
	// entirely rather than rendering a bogus negative limit.
	if key.RateLimit != nil && key.RateLimit.Requests != nil && *key.RateLimit.Requests >= 0 {
		meters = append(meters, UsageMeter{
			Label: "Requests",
			Limit: strPtr(formatAmount(float64(*key.RateLimit.Requests))),
			Unit:  "requests",
		})
	}

	snap := &UsageSnapshot{
		ProviderID: openRouterProviderID,
		Meters:     meters,
		Currency:   strPtr("USD"),
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}
	// Total credits purchased; balance = credits - usage.
	if key.TotalCredits != nil && key.Usage != nil {
		snap.Balance = strPtr(formatAmount(*key.TotalCredits - *key.Usage))
	}
	return snap, nil
}

type openRouterKeyResponse struct {
	Data openRouterKeyData `json:"data"`
}

type openRouterKeyData struct {
	Label        *string                `json:"label"`
	Usage        *float64               `json:"usage"`
	Limit        *float64               `json:"limit"`
	IsFreeTier   *bool                  `json:"is_free_tier"`
	RateLimit    *openRouterRateLimit   `json:"rate_limit"`
	UsagePeriod  *openRouterUsagePeriod `json:"usage_period"`
	TotalCredits *float64               `json:"total_credits"`
}

type openRouterRateLimit struct {
	Requests *int    `json:"requests"`
	Interval *string `json:"interval"`
}

type openRouterUsagePeriod struct {
	StartTime *time.Time `json:"start_time"`
	EndTime   *time.Time `json:"end_time"`
}

func (p *openRouterUsagePeriod) endTime() *time.Time {
	if p == nil {
		return nil
	}
	return p.EndTime
}
