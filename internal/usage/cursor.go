package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"time"
)

const (
	cursorProviderID  = "cursor"
	cursorDisplayName = "Cursor"
)

// CursorProvider — Cursor usage provider.
//
// Cursor is web-backed: quota comes from cursor.com endpoints with the
// browser session. This pass ports only the web/cookie strategy
// (CURSOR_COOKIE env var). The Swift original's second source — deriving a
// session from Cursor.app's own local state.vscdb (a SQLite database) — is
// not ported: reading it needs a SQLite library, and this ecosystem keeps
// cross-compiled Go binaries CGO-free, so that's a dependency decision
// (CGO vs. a pure-Go/WASM SQLite reader) for its own follow-up, not a
// silent addition here. Mirrors symaira-cockpit's CursorUsageProvider (web
// path only).
type CursorProvider struct {
	cookieHeader string
	client       *http.Client
}

// NewCursorProvider reads the cookie from CURSOR_COOKIE.
func NewCursorProvider(client *http.Client) *CursorProvider {
	if client == nil {
		client = http.DefaultClient
	}
	return &CursorProvider{cookieHeader: os.Getenv("CURSOR_COOKIE"), client: client}
}

func (p *CursorProvider) ID() string          { return cursorProviderID }
func (p *CursorProvider) DisplayName() string { return cursorDisplayName }
func (p *CursorProvider) IsConfigured() bool  { return p.cookieHeader != "" }

func (p *CursorProvider) Strategies() []Strategy {
	if p.cookieHeader == "" {
		return nil
	}
	return []Strategy{&cursorWebStrategy{cookieHeader: p.cookieHeader, client: p.client}}
}

func (p *CursorProvider) AuthStatus() AuthStatus {
	if p.cookieHeader != "" {
		return AuthStatus{Status: "available", Detail: "Cookie configured (CURSOR_COOKIE)", Source: "env"}
	}
	return AuthStatus{Status: "missing", Detail: "No Cursor credentials found — set CURSOR_COOKIE or sign in to Cursor.app"}
}

// cursorError mirrors the Swift original's CursorError.
type cursorError struct {
	kind   string // invalid_credentials | network | status | parse_failed
	status int
	detail string
}

func (e *cursorError) Error() string {
	switch e.kind {
	case "invalid_credentials":
		return "Cursor session is invalid or expired. Re-import the cursor.com cookie or sign in to Cursor.app."
	case "network":
		return fmt.Sprintf("Cursor request failed: %s", e.detail)
	case "status":
		return fmt.Sprintf("Cursor request failed with HTTP %d", e.status)
	default:
		return fmt.Sprintf("Cursor returned an unreadable response: %s", e.detail)
	}
}

// cursorWebStrategy fetches Cursor quota with a cursor.com session cookie:
// GET https://cursor.com/api/usage-summary — plan usage, on-demand usage,
// and the billing-cycle reset.
type cursorWebStrategy struct {
	cookieHeader string
	client       *http.Client
}

func (s *cursorWebStrategy) Source() string { return "web" }

func (s *cursorWebStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://cursor.com/api/usage-summary", nil)
	if err != nil {
		return nil, &cursorError{kind: "network", detail: err.Error()}
	}
	req.Header.Set("Cookie", s.cookieHeader)
	req.Header.Set("Accept", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, &cursorError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		switch resp.StatusCode {
		case 401, 403:
			return nil, &cursorError{kind: "invalid_credentials"}
		case 429:
			return nil, &RateLimitedError{ProviderID: cursorProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		default:
			return nil, &cursorError{kind: "status", status: resp.StatusCode}
		}
	}

	var summary cursorUsageSummary
	if err := json.NewDecoder(resp.Body).Decode(&summary); err != nil {
		return nil, &cursorError{kind: "parse_failed", detail: "usage summary is not JSON"}
	}
	return cursorSnapshot(summary), nil
}

// cursorSnapshot normalizes the usage summary into meters:
//   - Plan usage (included) as a percent (or USD when no percent given)
//   - Auto and API shares as percents, when reported
//   - On-demand / extra usage as USD
//   - Reset = billing cycle end
func cursorSnapshot(summary cursorUsageSummary) *UsageSnapshot {
	var meters []UsageMeter
	var reset *time.Time
	if summary.BillingCycleEnd != nil {
		if t, err := time.Parse(time.RFC3339, *summary.BillingCycleEnd); err == nil {
			reset = &t
		}
	}

	if plan := summary.IndividualUsage.plan(); plan != nil && !boolFalse(plan.Enabled) {
		switch {
		case plan.TotalPercentUsed != nil:
			meters = append(meters, UsageMeter{
				Label: "Plan usage", Used: strPtr(formatAmount(clampPercent(*plan.TotalPercentUsed))),
				Limit: strPtr("100"), Unit: "%", ResetsAt: reset,
			})
		case plan.Used != nil && plan.Limit != nil && *plan.Limit > 0:
			meters = append(meters, UsageMeter{
				Label: "Plan usage", Used: strPtr(formatAmount(float64(*plan.Used))),
				Limit: strPtr(formatAmount(float64(*plan.Limit))), Unit: "USD", ResetsAt: reset,
			})
		}
		if plan.AutoPercentUsed != nil {
			meters = append(meters, UsageMeter{
				Label: "Auto usage", Used: strPtr(formatAmount(clampPercent(*plan.AutoPercentUsed))),
				Limit: strPtr("100"), Unit: "%", ResetsAt: reset,
			})
		}
		if plan.APIPercentUsed != nil {
			meters = append(meters, UsageMeter{
				Label: "API usage", Used: strPtr(formatAmount(clampPercent(*plan.APIPercentUsed))),
				Limit: strPtr("100"), Unit: "%", ResetsAt: reset,
			})
		}
	}

	if onDemand := summary.IndividualUsage.onDemand(); onDemand != nil && !boolFalse(onDemand.Enabled) {
		if onDemand.Used != nil {
			m := UsageMeter{Label: "On-demand usage", Used: strPtr(formatAmount(float64(*onDemand.Used))), Unit: "USD", ResetsAt: reset}
			if onDemand.Limit != nil && *onDemand.Limit > 0 {
				m.Limit = strPtr(formatAmount(float64(*onDemand.Limit)))
			}
			meters = append(meters, m)
		}
	}

	return &UsageSnapshot{
		ProviderID: cursorProviderID,
		Meters:     meters,
		FetchedAt:  time.Now().UTC(),
		Source:     "web",
	}
}

func boolFalse(b *bool) bool { return b != nil && !*b }

// clampPercent converts a 0..1 fraction to a 0..100 percentage, clamped.
func clampPercent(fraction float64) float64 {
	v := fraction * 100
	if v < 0 {
		return 0
	}
	if v > 100 {
		return 100
	}
	return v
}

// cursorUsageSummary — GET /api/usage-summary. Money values are in cents in
// the real API but this port passes them through as reported (matching the
// Swift original, which also does not divide by 100 — the unit is simply
// USD-labeled raw numbers as returned).
type cursorUsageSummary struct {
	BillingCycleEnd *string               `json:"billingCycleEnd"`
	IndividualUsage cursorIndividualUsage `json:"individualUsage"`
}

type cursorIndividualUsage struct {
	Plan     *cursorPlanUsage     `json:"plan"`
	OnDemand *cursorOnDemandUsage `json:"onDemand"`
}

func (u cursorIndividualUsage) plan() *cursorPlanUsage         { return u.Plan }
func (u cursorIndividualUsage) onDemand() *cursorOnDemandUsage { return u.OnDemand }

type cursorPlanUsage struct {
	Enabled          *bool    `json:"enabled"`
	Used             *int     `json:"used"`
	Limit            *int     `json:"limit"`
	AutoPercentUsed  *float64 `json:"autoPercentUsed"`
	APIPercentUsed   *float64 `json:"apiPercentUsed"`
	TotalPercentUsed *float64 `json:"totalPercentUsed"`
}

type cursorOnDemandUsage struct {
	Enabled *bool `json:"enabled"`
	Used    *int  `json:"used"`
	Limit   *int  `json:"limit"`
}
