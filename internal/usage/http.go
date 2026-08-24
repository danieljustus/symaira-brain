package usage

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
)

// FetchJSON performs req via client, maps the HTTP status the same way
// symaira-cockpit's AIUsageHTTP.json does — 401/403 -> NotConfiguredError,
// 429 -> RateLimitedError with Retry-After parsing, every other non-2xx ->
// HTTPError{Kind:"status"} — and decodes the 2xx body as T. Request
// construction (URL, method, auth headers) stays with the caller.
func FetchJSON[T any](ctx context.Context, req *http.Request, providerID string, client *http.Client) (T, error) {
	var zero T
	req = req.Clone(ctx)
	if req.Header.Get("Accept") == "" {
		req.Header.Set("Accept", "application/json")
	}

	resp, err := client.Do(req)
	if err != nil {
		return zero, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		switch resp.StatusCode {
		case 401, 403:
			return zero, &NotConfiguredError{ProviderID: providerID}
		case 429:
			return zero, &RateLimitedError{ProviderID: providerID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		default:
			return zero, &HTTPError{Kind: "status", Status: resp.StatusCode}
		}
	}

	var out T
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return zero, &HTTPError{Kind: "unparseable"}
	}
	return out, nil
}

// parseRetryAfter parses the Retry-After header's delta-seconds form (e.g.
// "30"); nil when the header is absent or not a plain number — the
// HTTP-date form is not handled, matching the Swift original (429 responses
// conventionally use delta-seconds).
func parseRetryAfter(value string) *float64 {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}
	seconds, err := strconv.ParseFloat(value, 64)
	if err != nil {
		return nil
	}
	return &seconds
}
