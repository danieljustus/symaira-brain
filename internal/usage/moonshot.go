package usage

import (
	"context"
	"net/http"
	"os"
	"time"
)

const (
	moonshotProviderID  = "moonshot"
	moonshotDisplayName = "Moonshot"
)

// MoonshotRegion selects the Moonshot (Kimi Open Platform) host and
// currency. Raw values match the Swift original's MOONSHOT_REGION values
// verbatim ("ai", "cn") so an env var set for the Swift build behaves
// identically here.
type MoonshotRegion string

const (
	MoonshotRegionInternational MoonshotRegion = "ai"
	MoonshotRegionChina         MoonshotRegion = "cn"
)

func (r MoonshotRegion) host() string {
	if r == MoonshotRegionChina {
		return "api.moonshot.cn"
	}
	return "api.moonshot.ai"
}

func (r MoonshotRegion) currency() string {
	if r == MoonshotRegionChina {
		return "CNY"
	}
	return "USD"
}

// MoonshotProvider — Moonshot (Kimi Open Platform) usage provider.
//
// Pay-as-you-go balance reported by GET /v1/users/me/balance; no session or
// weekly quota windows. Region-dependent host: api.moonshot.ai
// (international, USD) or api.moonshot.cn (China mainland, CNY); currency
// amounts are never added across currency boundaries. Credential resolution
// for this pass is MOONSHOT_API_KEY only (see OpenRouterProvider's doc for
// why). Mirrors symaira-cockpit's MoonshotUsageProvider.
type MoonshotProvider struct {
	apiKey     string
	credErr    error
	credSource string
	region     MoonshotRegion
	client     *http.Client
}

// NewMoonshotProvider reads MOONSHOT_API_KEY (symvault:// URIs accepted)
// and the MOONSHOT_REGION override ("ai" default, or "cn") from the
// environment.
func NewMoonshotProvider(client *http.Client) *MoonshotProvider {
	apiKey, credSource, credErr := resolveEnv("MOONSHOT_API_KEY")
	if client == nil {
		client = http.DefaultClient
	}
	region := MoonshotRegionInternational
	if MoonshotRegion(os.Getenv("MOONSHOT_REGION")) == MoonshotRegionChina {
		region = MoonshotRegionChina
	}
	return &MoonshotProvider{
		apiKey:     apiKey,
		credErr:    credErr,
		credSource: credSource,
		region:     region,
		client:     client,
	}
}

func (p *MoonshotProvider) ID() string          { return moonshotProviderID }
func (p *MoonshotProvider) DisplayName() string { return moonshotDisplayName }
func (p *MoonshotProvider) IsConfigured() bool  { return p.apiKey != "" }

func (p *MoonshotProvider) Strategies() []Strategy {
	if p.apiKey == "" {
		return nil
	}
	return []Strategy{&moonshotAPIStrategy{apiKey: p.apiKey, region: p.region, client: p.client}}
}

func (p *MoonshotProvider) AuthStatus() AuthStatus {
	if p.credErr != nil {
		return authErrStatus(p.credErr)
	}
	if p.apiKey == "" {
		return AuthStatus{Status: "missing", Detail: "no API key configured (MOONSHOT_API_KEY)"}
	}
	return AuthStatus{Status: "available", Detail: "API key from MOONSHOT_API_KEY", Source: p.credSource}
}

// moonshotAPIStrategy fetches the Moonshot pay-as-you-go balance:
// GET {host}/v1/users/me/balance with Authorization: Bearer <apiKey>.
type moonshotAPIStrategy struct {
	apiKey string
	region MoonshotRegion
	client *http.Client
}

func (s *moonshotAPIStrategy) Source() string { return "api" }

func (s *moonshotAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	url := "https://" + s.region.host() + "/v1/users/me/balance"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.apiKey)

	payload, err := FetchJSON[moonshotBalanceResponse](ctx, req, moonshotProviderID, s.client)
	if err != nil {
		return nil, err
	}

	currency := s.region.currency()
	var meters []UsageMeter
	if cash, ok := parseOptionalFloat(payload.CashBalance); ok {
		meters = append(meters, UsageMeter{Label: "Cash balance", Used: strPtr(formatAmount(cash)), Unit: currency})
	}
	if voucher, ok := parseOptionalFloat(payload.VoucherBalance); ok {
		meters = append(meters, UsageMeter{Label: "Voucher balance", Used: strPtr(formatAmount(voucher)), Unit: currency})
	}

	snap := &UsageSnapshot{
		ProviderID: moonshotProviderID,
		Meters:     meters,
		Currency:   strPtr(currency),
		FetchedAt:  time.Now().UTC(),
		Source:     s.Source(),
	}
	if avail, ok := parseOptionalFloat(payload.AvailableBalance); ok {
		snap.Balance = strPtr(formatAmount(avail))
	}
	return snap, nil
}

// moonshotBalanceResponse — Moonshot returns balances as decimal strings
// ("42.50"), not JSON numbers.
type moonshotBalanceResponse struct {
	AvailableBalance *string `json:"available_balance"`
	VoucherBalance   *string `json:"voucher_balance"`
	CashBalance      *string `json:"cash_balance"`
}
