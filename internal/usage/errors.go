package usage

import (
	"fmt"
	"strings"
	"time"
)

// NotConfiguredError means the provider has no usable credential.
type NotConfiguredError struct{ ProviderID string }

func (e *NotConfiguredError) Error() string {
	return fmt.Sprintf("AI usage provider %q is not configured", e.ProviderID)
}

// RateLimitedError means the provider's API rejected the request as rate
// limited. RetryAfterSeconds is nil when no delta-seconds header was usable.
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

// TimeoutError means a provider's whole strategy chain exceeded its budget.
type TimeoutError struct {
	ProviderID string
	Timeout    time.Duration
}

func (e *TimeoutError) Error() string {
	return fmt.Sprintf("AI usage provider %q timed out after %s", e.ProviderID, e.Timeout)
}

// PayloadError means a successful response did not contain usable usage data.
type PayloadError struct {
	ProviderID string
	Detail     string
}

func (e *PayloadError) Error() string {
	return fmt.Sprintf("AI usage provider %q returned malformed usage data: %s", e.ProviderID, e.Detail)
}

// ChainFailedError means every fallback strategy failed.
type ChainFailedError struct{ Failures []string }

func (e *ChainFailedError) Error() string {
	return fmt.Sprintf("all AI usage fallbacks failed: %s", strings.Join(e.Failures, "; "))
}

// HTTPError is the shared error for the AI usage providers' HTTP layer. Body
// contents are deliberately never included because an error page may echo
// request headers back, including a provider credential.
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
