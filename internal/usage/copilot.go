package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"sort"
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
	credErr        error
	credSource     string
	enterpriseHost string // "" = github.com
	client         *http.Client
}

// NewCopilotProvider reads the token from the local Copilot config
// (~/.config/github-copilot/{apps,hosts}.json).
func NewCopilotProvider(client *http.Client) *CopilotProvider {
	if client == nil {
		client = http.DefaultClient
	}
	accessToken, credSource, credErr := resolveFileCredential("COPILOT_ACCESS_TOKEN", func() string {
		return readCopilotToken(copilotConfigDir())
	})
	return &CopilotProvider{
		accessToken: accessToken,
		credErr:     credErr,
		credSource:  credSource,
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
		return AuthStatus{Status: "available", Detail: "Signed in via GitHub Copilot (COPILOT_ACCESS_TOKEN or Copilot CLI)", Source: p.credSource}
	}
	if p.credErr != nil {
		return authErrStatus(p.credErr)
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
			return fmt.Sprintf("Copilot rejected the login (HTTP %d). Re-authenticate with GitHub Copilot.", e.status)
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

	return &UsageSnapshot{
		ProviderID: copilotProviderID,
		Meters:     payload.meters(),
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}, nil
}

// Two generations of the copilot_internal/user payload are accepted. The
// current one reports every quota under `quota_snapshots`, keyed by quota
// id, with a shared reset date on the envelope. The older one carried a
// single premium-request counter under `copilot.chat` — that shape no longer
// comes back from github.com, but the endpoint is undocumented, so it stays
// parseable rather than being deleted.
type copilotUserResponse struct {
	QuotaSnapshots    map[string]copilotQuotaSnapshot `json:"quota_snapshots"`
	QuotaResetDate    string                          `json:"quota_reset_date"`
	QuotaResetDateUTC *time.Time                      `json:"quota_reset_date_utc"`
	// Legacy shape.
	Copilot    *copilotState      `json:"copilot"`
	SkillsChat *copilotSkillsChat `json:"skills_chat"`
}

// copilotQuotaOrder is the order quotas are rendered in: the one that
// actually runs out first, then the two that are unlimited on most plans.
// Anything the endpoint adds later follows, sorted, so a new quota shows up
// without a release here.
var copilotQuotaOrder = []string{"premium_interactions", "chat", "completions"}

var copilotQuotaLabels = map[string]string{
	"premium_interactions": "Premium requests",
	"chat":                 "Chat",
	"completions":          "Completions",
}

func (r copilotUserResponse) meters() []UsageMeter {
	if meters := r.quotaSnapshotMeters(); len(meters) > 0 {
		return meters
	}
	return r.legacyMeters()
}

func (r copilotUserResponse) quotaSnapshotMeters() []UsageMeter {
	if len(r.QuotaSnapshots) == 0 {
		return nil
	}
	resetsAt := r.resetsAt()
	var meters []UsageMeter
	for _, id := range copilotQuotaIDs(r.QuotaSnapshots) {
		if meter := r.QuotaSnapshots[id].meter(id, resetsAt); meter != nil {
			meters = append(meters, *meter)
		}
	}
	return meters
}

func copilotQuotaIDs(snapshots map[string]copilotQuotaSnapshot) []string {
	ids := make([]string, 0, len(snapshots))
	for _, id := range copilotQuotaOrder {
		if _, ok := snapshots[id]; ok {
			ids = append(ids, id)
		}
	}
	var extra []string
	for id := range snapshots {
		if !slices.Contains(copilotQuotaOrder, id) {
			extra = append(extra, id)
		}
	}
	sort.Strings(extra)
	return append(ids, extra...)
}

// resetsAt prefers the timestamp form and falls back to parsing the plain
// date, which is all older responses carry.
func (r copilotUserResponse) resetsAt() *time.Time {
	if r.QuotaResetDateUTC != nil {
		return r.QuotaResetDateUTC
	}
	if r.QuotaResetDate == "" {
		return nil
	}
	reset, err := time.Parse("2006-01-02", r.QuotaResetDate)
	if err != nil {
		return nil
	}
	return &reset
}

func (r copilotUserResponse) legacyMeters() []UsageMeter {
	var meters []UsageMeter
	if r.Copilot != nil && r.Copilot.Chat != nil && r.Copilot.Chat.PremiumModelRequests != nil {
		premium := r.Copilot.Chat.PremiumModelRequests
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
	if r.SkillsChat != nil && r.SkillsChat.TotalPremiumRequestsUsed != nil {
		meters = append(meters, UsageMeter{
			Label: "Skills chat requests",
			Used:  strPtr(formatAmount(float64(*r.SkillsChat.TotalPremiumRequestsUsed))),
			Unit:  "requests",
		})
	}
	return meters
}

type copilotQuotaSnapshot struct {
	QuotaID          string   `json:"quota_id"`
	Entitlement      *float64 `json:"entitlement"`
	Remaining        *float64 `json:"remaining"`
	PercentRemaining *float64 `json:"percent_remaining"`
	Unlimited        bool     `json:"unlimited"`
	HasQuota         bool     `json:"has_quota"`
}

// meter renders one quota as a request count where the plan grants a
// countable entitlement, and as a percentage where it only reports how much
// is left. An unlimited quota is skipped: a bar that can never move says
// less than the absence of one, and the meter model has no "unlimited".
func (q copilotQuotaSnapshot) meter(id string, resetsAt *time.Time) *UsageMeter {
	if !q.HasQuota || q.Unlimited {
		return nil
	}
	label := copilotQuotaLabels[id]
	if label == "" {
		label = copilotQuotaLabel(id)
	}
	if q.Entitlement != nil && *q.Entitlement > 0 {
		remaining := 0.0
		if q.Remaining != nil {
			remaining = *q.Remaining
		}
		return &UsageMeter{
			Label:    label,
			Used:     strPtr(formatAmount(max(0, *q.Entitlement-remaining))),
			Limit:    strPtr(formatAmount(*q.Entitlement)),
			Unit:     "requests",
			ResetsAt: resetsAt,
		}
	}
	if q.PercentRemaining != nil {
		return &UsageMeter{
			Label:    label,
			Used:     strPtr(formatAmount(max(0, 100-*q.PercentRemaining))),
			Limit:    strPtr(formatAmount(100)),
			Unit:     "%",
			ResetsAt: resetsAt,
		}
	}
	return nil
}

// copilotQuotaLabel turns an unknown quota id into something readable
// ("agent_sessions" -> "Agent sessions") so a quota GitHub adds later is
// still legible without a release here.
func copilotQuotaLabel(id string) string {
	label := strings.ReplaceAll(id, "_", " ")
	if label == "" {
		return id
	}
	return strings.ToUpper(label[:1]) + label[1:]
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
