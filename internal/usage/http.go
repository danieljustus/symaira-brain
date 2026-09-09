package usage

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
)

// FetchJSON performs req via client, bounds the response, refuses untrusted
// URLs, maps provider HTTP status, and decodes the 2xx body as T. Request
// construction (URL, method, auth headers) stays with the caller.
func FetchJSON[T any](ctx context.Context, req *http.Request, providerID string, client *http.Client) (T, error) {
	var zero T
	if err := validateUsageRequest(req, false); err != nil {
		return zero, &HTTPError{Kind: "invalid_response"}
	}
	if client == nil {
		client = newProviderHTTPClient()
	}
	client = usageClientWithoutRedirects(client)
	req = req.Clone(ctx)
	if req.Header.Get("Accept") == "" {
		req.Header.Set("Accept", "application/json")
	}

	resp, err := client.Do(req) // #nosec G704 -- validateUsageRequest enforces trusted HTTPS origins and usage clients reject redirects.
	if err != nil {
		return zero, &HTTPError{Kind: "network", Detail: err.Error()}
	}
	body, err := readUsageBodyAndClose(resp.Body)
	if err != nil {
		return zero, &HTTPError{Kind: "unparseable", Detail: err.Error()}
	}

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
	if err := json.Unmarshal(body, &out); err != nil {
		return zero, &HTTPError{Kind: "unparseable"}
	}
	return out, nil
}

// parseRetryAfter parses the Retry-After header's delta-seconds form (e.g.
// "30"); nil when the header is absent or not a plain number.
func parseRetryAfter(value string) *float64 {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}
	seconds, err := strconv.ParseFloat(value, 64)
	if err != nil || seconds < 0 {
		return nil
	}
	return &seconds
}
