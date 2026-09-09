package usage

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
)

const (
	// maxUsageResponseBytes bounds every provider response before parsing.
	maxUsageResponseBytes = 1 << 20
	// maxUsageRequestBodyBytes bounds provider POST payloads.
	maxUsageRequestBodyBytes = 64 << 10
)

var errUsageResponseTooLarge = errors.New("provider response exceeds 1048576 bytes")

// newProviderHTTPClient is the only default HTTP client used by production
// usage providers. Redirects are returned to the caller rather than followed;
// credentials must never be forwarded to an unvalidated origin.
func newProviderHTTPClient() *http.Client {
	transport := http.DefaultTransport
	if base, ok := http.DefaultTransport.(*http.Transport); ok {
		transport = base.Clone()
	}
	return &http.Client{
		Transport: transport,
		Timeout:   providerFetchTimeout,
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
}

func usageClientWithoutRedirects(client *http.Client) *http.Client {
	clone := *client
	clone.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &clone
}

// readUsageBody reads at most maxUsageResponseBytes+1 bytes. The extra byte
// distinguishes an exactly-at-limit response from an oversized response.
func readUsageBody(r io.Reader) ([]byte, error) {
	body, err := io.ReadAll(io.LimitReader(r, maxUsageResponseBytes+1))
	if err != nil {
		return nil, err
	}
	if len(body) > maxUsageResponseBytes {
		return nil, errUsageResponseTooLarge
	}
	return body, nil
}

// readUsageBodyAndClose preserves a body read error over a close error while
// still checking Close after every bounded response read.
func readUsageBodyAndClose(body io.ReadCloser) ([]byte, error) {
	data, readErr := readUsageBody(body)
	closeErr := body.Close()
	if readErr != nil {
		return nil, readErr
	}
	if closeErr != nil {
		return nil, closeErr
	}
	return data, nil
}

// trustedHTTPSURL accepts only an HTTPS URL whose authority is not a local or
// private-network origin. Local providers must opt into loopback explicitly.
func trustedHTTPSURL(raw string, allowLoopback bool) bool {
	if raw == "" || strings.TrimSpace(raw) != raw || strings.ContainsAny(raw, "\r\n\x00") {
		return false
	}
	u, err := url.ParseRequestURI(raw)
	if err != nil || u.Scheme != "https" || u.Host == "" || u.User != nil || u.Fragment != "" {
		return false
	}
	if strings.ContainsAny(u.Host, "\r\n\x00") {
		return false
	}
	if port := u.Port(); port != "" {
		n, err := strconv.ParseUint(port, 10, 16)
		if err != nil || n == 0 {
			return false
		}
	} else if strings.Contains(u.Host, ":") && !strings.HasPrefix(u.Host, "[") {
		return false
	}

	host := strings.TrimSuffix(strings.ToLower(u.Hostname()), ".")
	if host == "" || host == "localhost" || strings.HasSuffix(host, ".localhost") ||
		host == "local" || strings.HasSuffix(host, ".local") ||
		strings.HasSuffix(host, ".internal") || strings.HasSuffix(host, ".home.arpa") {
		return false
	}
	if ip, err := netip.ParseAddr(host); err == nil {
		if allowLoopback {
			return ip.IsLoopback()
		}
		return !privateUsageIP(ip)
	}
	if allowLoopback {
		return false
	}
	return true
}

func privateUsageIP(ip netip.Addr) bool {
	if !ip.IsValid() {
		return true
	}
	if ip.Is4In6() {
		return privateUsageIP(ip.Unmap())
	}
	if ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() ||
		ip.IsUnspecified() || ip.IsMulticast() {
		return true
	}
	if ip.Is4() {
		v4 := ip.As4()
		if v4 == [4]byte{255, 255, 255, 255} || v4[0] == 0 || v4[0] == 100 && v4[1] >= 64 && v4[1] <= 127 ||
			v4[0] == 192 && v4[1] == 0 && (v4[2] == 0 || v4[2] == 2) ||
			v4[0] == 198 && v4[1] >= 18 && v4[1] <= 19 ||
			v4[0] == 198 && v4[1] == 51 && v4[2] == 100 ||
			v4[0] == 203 && v4[1] == 0 && v4[2] == 113 {
			return true
		}
	}
	return false
}

func validatedUsageBase(raw, fallback string) string {
	if trustedHTTPSURL(raw, false) {
		return strings.TrimSuffix(raw, "/")
	}
	return fallback
}

func validateUsageRequest(req *http.Request, allowLoopback bool) error {
	if req == nil || req.URL == nil || !trustedHTTPSURL(req.URL.String(), allowLoopback) {
		return fmt.Errorf("provider URL must use trusted HTTPS")
	}
	if req.Body != nil && req.ContentLength > maxUsageRequestBodyBytes {
		return fmt.Errorf("provider request body exceeds %d bytes", maxUsageRequestBodyBytes)
	}
	return nil
}

func doUsageRequest(ctx context.Context, client *http.Client, req *http.Request, allowLoopback bool) (*http.Response, error) {
	if err := validateUsageRequest(req, allowLoopback); err != nil {
		return nil, err
	}
	if client == nil {
		client = newProviderHTTPClient()
	}
	client = usageClientWithoutRedirects(client)
	return client.Do(req.Clone(ctx)) // #nosec G704 -- validateUsageRequest enforces trusted HTTPS origins and redirects are disabled.
}

// parseUsageJSON decodes a bounded response body.
func parseUsageJSON[T any](body io.Reader) (T, error) {
	var zero T
	data, err := readUsageBody(body)
	if err != nil {
		return zero, err
	}
	var out T
	if err := json.Unmarshal(data, &out); err != nil {
		return zero, err
	}
	return out, nil
}
