package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	copilotProviderID  = "copilot"
	copilotDisplayName = "GitHub Copilot"
)

// CopilotProvider — GitHub Copilot usage provider.
//
// Primary source: an existing Copilot OAuth token from the local Copilot
// config (~/.config/github-copilot/apps.json or hosts.json), then the
// copilot_internal/user usage endpoint. The GitHub Device Flow that Codex's
// Swift original exposes for interactive sign-in is intentionally not
// ported here: it is a user-initiated write action, not part of read-only
// usage reporting, and this pass only ports read paths. Mirrors
// symaira-cockpit's CopilotUsageProvider.
type CopilotProvider struct {
	accessToken    string
	enterpriseHost string // "" = github.com
	client         *http.Client
}

// NewCopilotProvider reads the token from the local Copilot config
// (~/.config/github-copilot/{apps,hosts}.json).
func NewCopilotProvider(client *http.Client) *CopilotProvider {
	if client == nil {
		client = http.DefaultClient
	}
	return &CopilotProvider{
		accessToken: readCopilotToken(copilotConfigDir()),
		client:      client,
	}
}

func copilotConfigDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "github-copilot")
}

// readCopilotToken reads oauth_token from apps.json (then hosts.json) in
// dir, preferring a github.com entry, falling back to any entry.
func readCopilotToken(dir string) string {
	for _, filename := range []string{"apps.json", "hosts.json"} {
		data, err := os.ReadFile(filepath.Join(dir, filename))
		if err != nil {
			continue
		}
		var root map[string]json.RawMessage
		if err := json.Unmarshal(data, &root); err != nil {
			continue
		}
		if token := copilotTokenFrom(root, "github.com:"); token != "" {
			return token
		}
		if token := copilotTokenFrom(root, ""); token != "" {
			return token
		}
	}
	return ""
}

func copilotTokenFrom(root map[string]json.RawMessage, hostPrefix string) string {
	for key, raw := range root {
		if hostPrefix != "" && !strings.HasPrefix(key, hostPrefix) {
			continue
		}
		var entry struct {
			OAuthToken string `json:"oauth_token"`
		}
		if err := json.Unmarshal(raw, &entry); err != nil {
			continue
		}
		if entry.OAuthToken != "" {
			return entry.OAuthToken
		}
	}
	return ""
}

func (p *CopilotProvider) ID() string          { return copilotProviderID }
func (p *CopilotProvider) DisplayName() string { return copilotDisplayName }
func (p *CopilotProvider) IsConfigured() bool  { return p.accessToken != "" }

func (p *CopilotProvider) Strategies() []Strategy {
	if p.accessToken == "" {
		return nil
	}
	host := p.enterpriseHost
	if host == "" {
		host = "github.com"
	}
	return []Strategy{&copilotAPIStrategy{accessToken: p.accessToken, host: host, client: p.client}}
}

func (p *CopilotProvider) AuthStatus() AuthStatus {
	if p.accessToken != "" {
		return AuthStatus{Status: "available", Detail: "Signed in via GitHub Copilot", Source: "file"}
	}
	return AuthStatus{Status: "missing", Detail: "No Copilot token found — sign in with the Copilot CLI"}
}

// copilotError mirrors the Swift original's CopilotError — unlike every
// other provider, a 401/403 here is reported as a plain HTTP-status error
// with a re-auth hint, not NotConfiguredError (matches the Swift original's
// distinct status handling for this one provider).
type copilotError struct {
	kind   string // network | invalid_response | status | unparseable
	status int
	detail string
}

func (e *copilotError) Error() string {
	switch e.kind {
	case "network":
		return fmt.Sprintf("Copilot request failed: %s", e.detail)
	case "invalid_response":
		return "Copilot returned an invalid response"
	case "status":
		if e.status == 401 || e.status == 403 {
			return fmt.Sprintf("Copilot rejected the token (HTTP %d). Re-authenticate with GitHub Copilot.", e.status)
		}
		return fmt.Sprintf("Copilot request failed with HTTP %d", e.status)
	default:
		return "Copilot returned an unreadable response"
	}
}

// copilotAPIStrategy fetches Copilot plan usage from the user endpoint:
// GET https://api.github.com/copilot_internal/user (or the enterprise
// host's /api/v3 base) with Authorization: Bearer <token>.
type copilotAPIStrategy struct {
	accessToken string
	host        string
	client      *http.Client
}

func (s *copilotAPIStrategy) Source() string { return "api" }

func (s *copilotAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	base := "https://api.github.com"
	if s.host != "github.com" {
		base = "https://" + s.host + "/api/v3"
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, base+"/copilot_internal/user", nil)
	if err != nil {
		return nil, &copilotError{kind: "network", detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.accessToken)
	req.Header.Set("Accept", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, &copilotError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: copilotProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &copilotError{kind: "status", status: resp.StatusCode}
	}

	var payload copilotUserResponse
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return nil, &copilotError{kind: "unparseable"}
	}

	var meters []UsageMeter
	// Premium-model requests quota as a meter with resetsAt.
	if payload.Copilot != nil && payload.Copilot.Chat != nil && payload.Copilot.Chat.PremiumModelRequests != nil {
		premium := payload.Copilot.Chat.PremiumModelRequests
		var used, limit *string
		if premium.TotalPremiumRequestsUsed != nil {
			used = strPtr(formatAmount(float64(*premium.TotalPremiumRequestsUsed)))
		}
		if premium.TotalPremiumRequestsIncluded != nil {
			limit = strPtr(formatAmount(float64(*premium.TotalPremiumRequestsIncluded)))
		}
		meters = append(meters, UsageMeter{
			Label:    "Premium requests",
			Used:     used,
			Limit:    limit,
			Unit:     "requests",
			ResetsAt: premium.UsageResetDate,
		})
	}
	if payload.SkillsChat != nil && payload.SkillsChat.TotalPremiumRequestsUsed != nil {
		meters = append(meters, UsageMeter{
			Label: "Skills chat requests",
			Used:  strPtr(formatAmount(float64(*payload.SkillsChat.TotalPremiumRequestsUsed))),
			Unit:  "requests",
		})
	}

	return &UsageSnapshot{
		ProviderID: copilotProviderID,
		Meters:     meters,
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

type copilotUserResponse struct {
	Copilot    *copilotState      `json:"copilot"`
	SkillsChat *copilotSkillsChat `json:"skills_chat"`
}

type copilotState struct {
	Chat *copilotChat `json:"chat"`
}

type copilotChat struct {
	PremiumModelRequests *copilotPremiumRequests `json:"premium_model_requests"`
}

type copilotPremiumRequests struct {
	TotalPremiumRequestsUsed     *int       `json:"total_premium_requests_used"`
	TotalPremiumRequestsIncluded *int       `json:"total_premium_requests_included"`
	UsageResetDate               *time.Time `json:"usage_reset_date"`
}

type copilotSkillsChat struct {
	TotalPremiumRequestsUsed *int `json:"total_premium_requests_used"`
}
