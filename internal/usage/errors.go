package usage

import (
	"fmt"
	"strings"
)

// NotConfiguredError means the provider has no usable credential.
// Mirrors symaira-cockpit's AIUsageError.notConfigured.
type NotConfiguredError struct{ ProviderID string }

func (e *NotConfiguredError) Error() string {
	return fmt.Sprintf("AI usage provider %q is not configured", e.ProviderID)
}

// RateLimitedError means the provider's API rejected the request as rate
// limited. RetryAfterSeconds is nil when the response carried no parseable
// Retry-After header. Mirrors AIUsageError.rateLimited.
type RateLimitedError struct {
	ProviderID        string
	RetryAfterSeconds *float64
}

func (e *RateLimitedError) Error() string {
	if e.RetryAfterSeconds != nil {
		return fmt.Sprintf("AI usage provider %q is rate limited; retry in %ds", e.ProviderID, int(*e.RetryAfterSeconds))
	}
	return fmt.Sprintf("AI usage provider %q is rate limited", e.ProviderID)
}

// ChainFailedError means every fallback strategy of a provider failed; each
// entry is one strategy's error message. Mirrors AIUsageError.chainFailed.
type ChainFailedError struct{ Failures []string }

func (e *ChainFailedError) Error() string {
	return fmt.Sprintf("all AI usage fallbacks failed: %s", strings.Join(e.Failures, "; "))
}

// HTTPError is the shared error for the AI usage providers' HTTP layer.
//
// The response body is deliberately never carried in any error — a gateway
// error page can echo the request headers back, including a provider's API
// key. Mirrors symaira-cockpit's AIUsageHTTPError.
type HTTPError struct {
	// Kind is one of: network, invalid_response, status, unparseable.
	Kind   string
	Status int
	Detail string
}

func (e *HTTPError) Error() string {
	switch e.Kind {
	case "network":
		return fmt.Sprintf("AI usage request failed: %s", e.Detail)
	case "invalid_response":
		return "AI usage provider returned an invalid response"
	case "status":
		return fmt.Sprintf("AI usage request failed with HTTP %d", e.Status)
	default:
		return "AI usage provider returned an unreadable response"
	}
}
