package usage

import (
	"context"
	"errors"
	"math"
	"net/http"
	"time"
)

// Credits are account-wide. Key periods/caps are optional and never replace lifetime spend.
type openRouterCreditsStrategy struct{ legacy *openRouterAPIStrategy }

func (s *openRouterCreditsStrategy) Source() string { return "api" }

func openRouterRequest(ctx context.Context, base, endpoint, key string) (*http.Request, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, base+endpoint, nil)
	if err != nil {
		return nil, &HTTPError{Kind: "invalid_response"}
	}
	req.Header.Set("Authorization", "Bearer "+key)
	req.Header.Set("X-Title", "symbrain")
	return req, nil
}

func (s *openRouterCreditsStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	l := s.legacy
	req, err := openRouterRequest(ctx, l.baseURL, "/credits", l.apiKey)
	if err != nil {
		return nil, err
	}
	payload, err := FetchJSON[openRouterCreditsResponse](ctx, req, openRouterProviderID, l.client)
	if err != nil {
		var httpErr *HTTPError
		// Only endpoint absence permits the old API, never authentication/schema/integrity failure.
		if errors.As(err, &httpErr) && httpErr.Kind == "status" && httpErr.Status == 404 {
			snap, err := l.Fetch(ctx)
			return snap, safeOpenRouterError(err)
		}
		return nil, safeOpenRouterError(err)
	}
	credits, usage := payload.Data.TotalCredits, payload.Data.TotalUsage
	if !validOpenRouterAmount(credits) || !validOpenRouterAmount(usage) {
		return nil, &PayloadError{ProviderID: openRouterProviderID, Detail: "missing or invalid account credits"}
	}
	snap := &UsageSnapshot{ProviderID: openRouterProviderID, Source: "api", Currency: strPtr("USD"), FetchedAt: time.Now().UTC(), Balance: strPtr(formatAmount(*credits - *usage)), Meters: []UsageMeter{
		{Label: "Lifetime credits", Used: strPtr(formatAmount(*credits)), Unit: "USD"},
		{Label: "Lifetime spend", Used: strPtr(formatAmount(*usage)), Unit: "USD"},
	}}
	req, err = openRouterRequest(ctx, l.baseURL, "/key", l.apiKey)
	if err != nil {
		return snap, nil
	}
	// Leave time to return valid credits before the outer provider deadline expires.
	budget := time.Second
	if deadline, ok := ctx.Deadline(); ok {
		remaining := time.Until(deadline) - 100*time.Millisecond
		if remaining <= 0 {
			return snap, nil
		}
		if remaining < budget {
			budget = remaining
		}
	}
	optionalCtx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	key, err := FetchJSON[openRouterCurrentKeyResponse](optionalCtx, req, openRouterProviderID, l.client)
	if err == nil {
		snap.Meters = append(snap.Meters, openRouterPeriodMeters(key.Data)...)
	}
	return snap, nil
}

func safeOpenRouterError(err error) error {
	var httpErr *HTTPError
	if errors.As(err, &httpErr) && httpErr.Kind == "network" {
		return &HTTPError{Kind: "network", Detail: "provider transport failed"}
	}
	return err
}

type openRouterCreditsResponse struct {
	Data struct {
		TotalCredits *float64 `json:"total_credits"`
		TotalUsage   *float64 `json:"total_usage"`
	} `json:"data"`
}

type openRouterCurrentKeyResponse struct {
	Data openRouterCurrentKeyData `json:"data"`
}
type openRouterCurrentKeyData struct {
	Daily     *float64             `json:"usage_daily"`
	Weekly    *float64             `json:"usage_weekly"`
	Monthly   *float64             `json:"usage_monthly"`
	Limit     *float64             `json:"limit"`
	Remaining *float64             `json:"limit_remaining"`
	RateLimit *openRouterRateLimit `json:"rate_limit"`
}

func validOpenRouterAmount(v *float64) bool {
	return v != nil && !math.IsInf(*v, 0) && !math.IsNaN(*v) && *v >= 0
}

func openRouterPeriodMeters(k openRouterCurrentKeyData) []UsageMeter {
	var meters []UsageMeter
	for _, period := range []struct {
		label  string
		amount *float64
	}{{"Today", k.Daily}, {"This Week", k.Weekly}, {"This Month", k.Monthly}} {
		if validOpenRouterAmount(period.amount) {
			meters = append(meters, UsageMeter{Label: period.label, Used: strPtr(formatAmount(*period.amount)), Unit: "USD"})
		}
	}
	if validOpenRouterAmount(k.Limit) {
		var used *string
		if validOpenRouterAmount(k.Remaining) && *k.Remaining <= *k.Limit {
			used = strPtr(formatAmount(*k.Limit - *k.Remaining))
		}
		meters = append(meters, UsageMeter{Label: "Key limit", Used: used, Limit: strPtr(formatAmount(*k.Limit)), Unit: "USD"})
	}
	if k.RateLimit != nil && k.RateLimit.Requests != nil && *k.RateLimit.Requests >= 0 {
		meters = append(meters, UsageMeter{Label: "Requests", Limit: strPtr(formatAmount(float64(*k.RateLimit.Requests))), Unit: "requests"})
	}
	return meters
}
