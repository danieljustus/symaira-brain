package usage

import (
	"context"
	"encoding/json"
	"fmt"
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

	now := time.Now().UTC()
	var meters []UsageMeter
	// Session (primary) and weekly (secondary) windows as separate meters.
	if payload.RateLimit != nil {
		meters = append(meters, payload.RateLimit.meters(now, "")...)
	}
	// Model-specific additional rate limits (e.g. Codex Spark).
	for _, extra := range payload.AdditionalRateLimits {
		meters = append(meters, extra.meters(now)...)
	}

	return &UsageSnapshot{
		ProviderID: codexProviderID,
		Meters:     meters,
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

// The wham payload has two generations of shape, and both are accepted:
// the current one reports a window as `used_percent` against an implicit
// 100% cap with a Unix `reset_at`, while the older one reported an explicit
// `utilized`/`limit` pair with an RFC-3339 `reset_date`. Fields from both
// live side by side here because the endpoint is undocumented and can move
// back; whichever pair is populated is the one that produces the meter.
type codexWhamUsage struct {
	RateLimit            *codexRateLimit            `json:"rate_limit"`
	AdditionalRateLimits []codexAdditionalRateLimit `json:"additional_rate_limits"`
}

type codexRateLimit struct {
	PrimaryWindow   *codexWindow `json:"primary_window"`
	SecondaryWindow *codexWindow `json:"secondary_window"`
}

// meters renders the primary and secondary windows, prefixing each label
// with prefix (used by the per-model additional limits, empty otherwise).
func (r *codexRateLimit) meters(now time.Time, prefix string) []UsageMeter {
	if r == nil {
		return nil
	}
	var meters []UsageMeter
	for _, window := range []struct {
		window   *codexWindow
		fallback string
	}{
		{r.PrimaryWindow, "Session"},
		{r.SecondaryWindow, "Weekly"},
	} {
		if meter := window.window.meter(now, prefix, window.fallback); meter != nil {
			meters = append(meters, *meter)
		}
	}
	return meters
}

type codexWindow struct {
	// Current shape: a percentage of an implicit 100% cap, a window length
	// in seconds, and a Unix reset timestamp.
	UsedPercent        *float64 `json:"used_percent"`
	LimitWindowSeconds *int64   `json:"limit_window_seconds"`
	ResetAfterSeconds  *int64   `json:"reset_after_seconds"`
	ResetAt            *int64   `json:"reset_at"`
	// Legacy shape.
	Window    string     `json:"window"`
	Utilized  *float64   `json:"utilized"`
	Limit     *float64   `json:"limit"`
	ResetDate *time.Time `json:"reset_date"`
}

func (w *codexWindow) meter(now time.Time, prefix, fallbackLabel string) *UsageMeter {
	if w == nil {
		return nil
	}
	var used, limit float64
	switch {
	case w.Limit != nil && *w.Limit > 0:
		limit = *w.Limit
		if w.Utilized != nil {
			used = *w.Utilized
		}
	case w.UsedPercent != nil:
		used, limit = *w.UsedPercent, 100
	default:
		// Neither generation reported anything meterable for this window.
		return nil
	}

	label := w.Window
	if label == "" {
		label = codexWindowLabel(w.LimitWindowSeconds, fallbackLabel)
	}
	if prefix != "" {
		label = prefix + " " + label
	}
	return &UsageMeter{
		Label:    label,
		Used:     strPtr(formatAmount(used)),
		Limit:    strPtr(formatAmount(limit)),
		Unit:     "%",
		ResetsAt: w.resetsAt(now),
	}
}

// resetsAt prefers an absolute timestamp in either generation's spelling and
// falls back to the relative countdown, so a meter keeps its "resets in …"
// line even when only `reset_after_seconds` comes back.
func (w *codexWindow) resetsAt(now time.Time) *time.Time {
	if w.ResetDate != nil {
		return w.ResetDate
	}
	if w.ResetAt != nil && *w.ResetAt > 0 {
		reset := time.Unix(*w.ResetAt, 0).UTC()
		return &reset
	}
	if w.ResetAfterSeconds != nil && *w.ResetAfterSeconds > 0 {
		reset := now.Add(time.Duration(*w.ResetAfterSeconds) * time.Second)
		return &reset
	}
	return nil
}

// codexWindowLabel names a window by its length, matching the short forms
// the endpoint used to send itself ("5h", "1w").
func codexWindowLabel(seconds *int64, fallback string) string {
	if seconds == nil || *seconds <= 0 {
		return fallback
	}
	switch value := *seconds; {
	case value%(7*86_400) == 0:
		return fmt.Sprintf("%dw", value/(7*86_400))
	case value%86_400 == 0:
		return fmt.Sprintf("%dd", value/86_400)
	case value%3_600 == 0:
		return fmt.Sprintf("%dh", value/3_600)
	default:
		return fmt.Sprintf("%dm", value/60)
	}
}

type codexAdditionalRateLimit struct {
	// Current shape: a named limit wrapping its own window pair.
	LimitName string          `json:"limit_name"`
	RateLimit *codexRateLimit `json:"rate_limit"`
	// Legacy shape: one flat window per entry.
	Window    string     `json:"window"`
	Title     *string    `json:"title"`
	Utilized  *float64   `json:"utilized"`
	Limit     *float64   `json:"limit"`
	ResetDate *time.Time `json:"reset_date"`
}

func (a codexAdditionalRateLimit) meters(now time.Time) []UsageMeter {
	if a.RateLimit != nil {
		return a.RateLimit.meters(now, a.LimitName)
	}
	if a.Limit == nil || *a.Limit <= 0 {
		return nil
	}
	label := a.Window
	if a.Title != nil {
		label = *a.Title
	}
	var used *string
	if a.Utilized != nil {
		used = strPtr(formatAmount(*a.Utilized))
	}
	return []UsageMeter{{
		Label:    label,
		Used:     used,
		Limit:    strPtr(formatAmount(*a.Limit)),
		Unit:     "%",
		ResetsAt: a.ResetDate,
	}}
}
