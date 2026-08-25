package usage

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	nousProviderID       = "nous"
	nousDisplayName      = "Nous Portal"
	nousDefaultPortalURL = "https://portal.nousresearch.com"
)

// NousPortalProvider reads credits from the Nous Portal account API.
//
// The Hermes CLI persists its auth state in ~/.hermes/auth.json (override:
// HERMES_HOME). Nous uses single-use refresh tokens and protects that store
// with cross-process file locking, so this provider opens it strictly
// read-only: it never refreshes, rotates, or writes anything back. An
// expired access token surfaces as "re-auth needed" instead of being
// refreshed behind the CLI's back. Mirrors symaira-cockpit's
// NousPortalUsageProvider.
type NousPortalProvider struct {
	accessToken   string
	credErr       error
	credSource    string
	portalBaseURL string
	client        *http.Client
}

// NewNousPortalProvider reads the access token from the Hermes CLI auth
// store and the HERMES_PORTAL_BASE_URL override from the environment.
func NewNousPortalProvider(client *http.Client) *NousPortalProvider {
	if client == nil {
		client = http.DefaultClient
	}
	baseURL := os.Getenv("HERMES_PORTAL_BASE_URL")
	if baseURL == "" {
		baseURL = nousDefaultPortalURL
	}
	accessToken, credSource, credErr := resolveFileCredential("NOUS_PORTAL_ACCESS_TOKEN", func() string {
		return readNousAccessToken(nousAuthStorePath())
	})
	return &NousPortalProvider{
		accessToken:   accessToken,
		credErr:       credErr,
		credSource:    credSource,
		portalBaseURL: baseURL,
		client:        client,
	}
}

func (p *NousPortalProvider) ID() string          { return nousProviderID }
func (p *NousPortalProvider) DisplayName() string { return nousDisplayName }
func (p *NousPortalProvider) IsConfigured() bool  { return p.accessToken != "" }

func (p *NousPortalProvider) Strategies() []Strategy {
	if p.accessToken == "" {
		return nil
	}
	return []Strategy{&nousPortalAPIStrategy{accessToken: p.accessToken, portalBaseURL: p.portalBaseURL, client: p.client}}
}

func (p *NousPortalProvider) AuthStatus() AuthStatus {
	if p.accessToken != "" {
		return AuthStatus{Status: "available", Detail: "Signed in via Hermes CLI auth store (NOUS_PORTAL_ACCESS_TOKEN or file)", Source: p.credSource}
	}
	if p.credErr != nil {
		return authErrStatus(p.credErr)
	}
	return AuthStatus{Status: "missing", Detail: "No Nous Portal credentials found — sign in with the Hermes CLI"}
}

// nousAuthStorePath resolves $HERMES_HOME/auth.json, falling back to
// ~/.hermes/auth.json.
func nousAuthStorePath() string {
	if home := os.Getenv("HERMES_HOME"); home != "" {
		return filepath.Join(home, "auth.json")
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".hermes", "auth.json")
}

// readNousAccessToken opens path with a plain read-only file read — no
// locks, no writes, no token rotation — and returns the Nous provider's
// access token (prefers the scoped invoke JWT), or "" when absent/expired.
//
// Shape: {"version": 1, "providers": [{"id": "nous", ...state...}]}.
func readNousAccessToken(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	var root struct {
		Providers []struct {
			ID          string `json:"id"`
			InvokeJWT   string `json:"invoke_jwt"`
			AccessToken string `json:"access_token"`
		} `json:"providers"`
	}
	if err := json.Unmarshal(data, &root); err != nil {
		return ""
	}
	for _, provider := range root.Providers {
		if provider.ID != "nous" {
			continue
		}
		// Prefer the scoped invoke JWT, then the access token.
		token := provider.InvokeJWT
		if token == "" {
			token = provider.AccessToken
		}
		if token == "" {
			return ""
		}
		// JWT-shaped tokens get an expiry check; plain tokens pass through.
		if strings.Contains(token, ".") && !nousJWTIsLive(token) {
			return "" // expired or unparseable → treat as re-auth needed
		}
		return token
	}
	return ""
}

// nousJWTIsLive reports whether an unverified JWT's payload segment carries
// a future exp claim. Any parse failure is treated as not-live (fail
// closed: an unparseable token is not usable).
func nousJWTIsLive(token string) bool {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return false
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return false
	}
	var claims struct {
		Exp *float64 `json:"exp"`
	}
	if err := json.Unmarshal(payload, &claims); err != nil || claims.Exp == nil {
		return false
	}
	return time.Unix(int64(*claims.Exp), 0).After(time.Now())
}

// nousPortalAPIStrategy fetches Portal credits via the account API:
// GET {base}/api/oauth/account with Authorization: Bearer <token>.
// Read-only; an expired token surfaces as a re-auth hint, never a refresh.
type nousPortalAPIStrategy struct {
	accessToken   string
	portalBaseURL string
	client        *http.Client
}

func (s *nousPortalAPIStrategy) Source() string { return "api" }

func (s *nousPortalAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	url := strings.TrimRight(s.portalBaseURL, "/") + "/api/oauth/account"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.accessToken)

	payload, err := FetchJSON[nousAccountResponse](ctx, req, nousProviderID, s.client)
	if err != nil {
		return nil, err
	}
	// A 401/403 from FetchJSON maps to NotConfiguredError: the invoke JWT is
	// no longer valid (re-auth needed) and we never refresh it.

	var meters []UsageMeter
	if access := payload.PaidServiceAccess; access != nil {
		if access.SubscriptionCreditsRemaining != nil {
			meters = append(meters, UsageMeter{Label: "Subscription credits", Used: strPtr(formatAmount(*access.SubscriptionCreditsRemaining)), Unit: "credits"})
		}
		if access.PurchasedCreditsRemaining != nil {
			meters = append(meters, UsageMeter{Label: "Purchased credits", Used: strPtr(formatAmount(*access.PurchasedCreditsRemaining)), Unit: "credits"})
		}
	}
	if sub := payload.Subscription; sub != nil {
		if sub.CreditsRemaining != nil {
			var limit *string
			if sub.MonthlyCredits != nil {
				limit = strPtr(formatAmount(*sub.MonthlyCredits))
			}
			meters = append(meters, UsageMeter{
				Label:    "Plan credits remaining",
				Used:     strPtr(formatAmount(*sub.CreditsRemaining)),
				Limit:    limit,
				Unit:     "credits",
				ResetsAt: sub.CurrentPeriodEnd,
			})
		}
		if sub.RolloverCredits != nil {
			meters = append(meters, UsageMeter{Label: "Rollover credits", Used: strPtr(formatAmount(*sub.RolloverCredits)), Unit: "credits"})
		}
	}

	snap := &UsageSnapshot{
		ProviderID: nousProviderID,
		Meters:     meters,
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}
	if access := payload.PaidServiceAccess; access != nil && access.TotalUsableCredits != nil {
		snap.Balance = strPtr(formatAmount(*access.TotalUsableCredits))
	}
	return snap, nil
}

type nousAccountResponse struct {
	Subscription      *nousSubscription      `json:"subscription"`
	PaidServiceAccess *nousPaidServiceAccess `json:"paid_service_access"`
}

type nousSubscription struct {
	Plan             *string    `json:"plan"`
	Tier             *int       `json:"tier"`
	MonthlyCharge    *float64   `json:"monthly_charge"`
	MonthlyCredits   *float64   `json:"monthly_credits"`
	CurrentPeriodEnd *time.Time `json:"current_period_end"`
	CreditsRemaining *float64   `json:"credits_remaining"`
	RolloverCredits  *float64   `json:"rollover_credits"`
}

type nousPaidServiceAccess struct {
	Allowed                      *bool    `json:"allowed"`
	PaidAccess                   *bool    `json:"paid_access"`
	HasActiveSubscription        *bool    `json:"has_active_subscription"`
	SubscriptionCreditsRemaining *float64 `json:"subscription_credits_remaining"`
	PurchasedCreditsRemaining    *float64 `json:"purchased_credits_remaining"`
	TotalUsableCredits           *float64 `json:"total_usable_credits"`
}
