package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
)

const (
	kimiProviderID     = "kimi"
	kimiDisplayName    = "Kimi Code"
	kimiDefaultAPIBase = "https://api.kimi.com"
	kimiWebUsagesURL   = "https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages"
)

// KimiProvider — Kimi For Coding usage provider.
//
// Tracks the Kimi Code subscription quota (weekly request pool plus the
// 5-hour rate-limit window) via GET https://api.kimi.com/coding/v1/usages.
// Distinct from the Moonshot/Kimi Open Platform balance (MoonshotProvider).
//
// Fallback chain (first success wins): api (KIMI_CODE_API_KEY env) -> cli
// (Kimi Code CLI's own credential file, read-only) -> web
// (KIMI_AUTH_TOKEN env). All three sources are portable (env vars and
// plain file reads); the Keychain fallback the Swift original also has for
// api/web is not ported, consistent with every other provider in this pass.
// Mirrors symaira-cockpit's KimiUsageProvider.
type KimiProvider struct {
	apiKey         string
	apiErr         error
	apiSource      string
	cliAccessToken string
	cliDeviceID    string
	authToken      string
	authErr        error
	authSource     string
	baseURL        string
	client         *http.Client
}

// NewKimiProvider reads KIMI_CODE_API_KEY, the Kimi Code CLI's own
// credential file ($KIMI_CODE_HOME, ~/.kimi-code, or legacy ~/.kimi), and
// KIMI_AUTH_TOKEN from the environment.
func NewKimiProvider(client *http.Client) *KimiProvider {
	if client == nil {
		client = http.DefaultClient
	}
	cliHome := kimiCLIHome()
	store := kimiCLICredentialStore{home: cliHome}
	baseURL := os.Getenv("KIMI_CODE_BASE_URL")
	if baseURL == "" {
		baseURL = kimiDefaultAPIBase
	}
	apiKey, apiSource, apiErr := resolveEnv("KIMI_CODE_API_KEY")
	authToken, authSource, authErr := resolveEnv("KIMI_AUTH_TOKEN")
	return &KimiProvider{
		apiKey:         apiKey,
		apiErr:         apiErr,
		apiSource:      apiSource,
		cliAccessToken: store.readAccessToken(),
		cliDeviceID:    store.readDeviceID(),
		authToken:      authToken,
		authErr:        authErr,
		authSource:     authSource,
		baseURL:        baseURL,
		client:         client,
	}
}

// kimiCLIHome resolves $KIMI_CODE_HOME, else prefers the current ~/.kimi-code
// layout when its credential file exists, else falls back to legacy
// ~/.kimi, else ~/.kimi-code as the default path.
func kimiCLIHome() string {
	if home := os.Getenv("KIMI_CODE_HOME"); home != "" {
		return home
	}
	homeDir, _ := os.UserHomeDir()
	current := filepath.Join(homeDir, ".kimi-code")
	if _, err := os.Stat(filepath.Join(current, "credentials", "kimi-code.json")); err == nil {
		return current
	}
	legacy := filepath.Join(homeDir, ".kimi")
	if _, err := os.Stat(filepath.Join(legacy, "credentials", "kimi-code.json")); err == nil {
		return legacy
	}
	return current
}

func (p *KimiProvider) ID() string          { return kimiProviderID }
func (p *KimiProvider) DisplayName() string { return kimiDisplayName }
func (p *KimiProvider) IsConfigured() bool {
	return p.apiKey != "" || p.cliAccessToken != "" || p.authToken != ""
}

func (p *KimiProvider) Strategies() []Strategy {
	var strategies []Strategy
	if p.apiKey != "" {
		strategies = append(strategies, &kimiAPIStrategy{apiKey: p.apiKey, baseURL: p.baseURL, client: p.client})
	}
	if p.cliAccessToken != "" {
		strategies = append(strategies, &kimiCLIStrategy{
			accessToken:     p.cliAccessToken,
			identityHeaders: kimiCLIIdentityHeaders(p.cliDeviceID),
			baseURL:         p.baseURL,
			client:          p.client,
		})
	}
	if p.authToken != "" {
		strategies = append(strategies, &kimiWebStrategy{authToken: p.authToken, client: p.client})
	}
	return strategies
}

func (p *KimiProvider) AuthStatus() AuthStatus {
	if p.cliAccessToken == "" && p.apiKey == "" && p.authToken == "" {
		if p.apiErr != nil {
			return authErrStatus(p.apiErr)
		}
		if p.authErr != nil {
			return authErrStatus(p.authErr)
		}
		return AuthStatus{Status: "missing", Detail: "No Kimi Code CLI credentials found"}
	}
	if p.cliAccessToken != "" {
		return AuthStatus{Status: "available", Detail: "Kimi Code CLI is signed in", Source: "cli"}
	}
	if p.apiKey != "" {
		return AuthStatus{Status: "available", Detail: "API key from KIMI_CODE_API_KEY", Source: p.apiSource}
	}
	return AuthStatus{Status: "available", Detail: "Web auth token from KIMI_AUTH_TOKEN", Source: p.authSource}
}

// kimiCLICredentialStore reads the Kimi Code CLI credential file and device
// id, strictly read-only — never writes, never creates missing files,
// never touches the refresh token.
type kimiCLICredentialStore struct{ home string }

func (s kimiCLICredentialStore) readAccessToken() string {
	data, err := os.ReadFile(filepath.Join(s.home, "credentials", "kimi-code.json"))
	if err != nil {
		return ""
	}
	var root struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.Unmarshal(data, &root); err != nil {
		return ""
	}
	return root.AccessToken
}

func (s kimiCLICredentialStore) readDeviceID() string {
	data, err := os.ReadFile(filepath.Join(s.home, "device_id"))
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

// kimiCLIIdentityHeaders builds the X-Msh-* headers the official Kimi Code
// CLI sends alongside its token.
func kimiCLIIdentityHeaders(deviceID string) map[string]string {
	hostName, _ := os.Hostname()
	headers := map[string]string{
		"X-Msh-Platform":    kimiPlatformLabel(),
		"X-Msh-Device-Name": hostName,
	}
	if deviceID != "" {
		headers["X-Msh-Device-Id"] = deviceID
	}
	version := kimiOSVersion()
	headers["X-Msh-Os-Version"] = version
	headers["X-Msh-Device-Model"] = kimiPlatformDisplayName() + " " + version
	return headers
}

func kimiPlatformLabel() string {
	switch runtime.GOOS {
	case "darwin":
		return "macos"
	default:
		return runtime.GOOS
	}
}

func kimiPlatformDisplayName() string {
	switch runtime.GOOS {
	case "darwin":
		return "macOS"
	default:
		return runtime.GOOS
	}
}

// kimiOSVersion is best-effort: the Swift original reads the precise OS
// version (ProcessInfo.operatingSystemVersion); Go has no portable
// equivalent, so this reports the Go runtime version instead — the header
// is informational identity metadata, not something the endpoint gates on.
func kimiOSVersion() string {
	return strings.TrimPrefix(runtime.Version(), "go")
}

type kimiError struct {
	kind   string // network | invalid_response | status | unparseable
	status int
	detail string
}

func (e *kimiError) Error() string {
	switch e.kind {
	case "network":
		return fmt.Sprintf("Kimi request failed: %s", e.detail)
	case "invalid_response":
		return "Kimi returned an invalid response"
	case "status":
		if e.status == 401 || e.status == 403 {
			return fmt.Sprintf("Kimi rejected the login (HTTP %d). Check the API access or sign in with the Kimi Code CLI again.", e.status)
		}
		return fmt.Sprintf("Kimi request failed with HTTP %d", e.status)
	default:
		return "Kimi returned an unreadable response"
	}
}

// kimiPerformGET performs the shared GET against the Kimi Code usage
// endpoint (coding/v1/usages) with an Authorization bearer token and
// optional identity headers.
func kimiPerformGET(ctx context.Context, baseURL, token string, identityHeaders map[string]string, client *http.Client) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, strings.TrimRight(baseURL, "/")+"/coding/v1/usages", nil)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	for name, value := range identityHeaders {
		req.Header.Set(name, value)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")

	resp, err := client.Do(req)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: kimiProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &kimiError{kind: "status", status: resp.StatusCode}
	}
	return kimiReadAll(resp)
}

func kimiReadAll(resp *http.Response) ([]byte, error) {
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	return data, nil
}

// kimiAPIStrategy fetches Kimi Code usage with an API key.
type kimiAPIStrategy struct {
	apiKey  string
	baseURL string
	client  *http.Client
}

func (s *kimiAPIStrategy) Source() string { return "api" }

func (s *kimiAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	data, err := kimiPerformGET(ctx, s.baseURL, s.apiKey, nil, s.client)
	if err != nil {
		return nil, err
	}
	return kimiSnapshotFromUsageResponse(data, s.Source())
}

// kimiCLIStrategy fetches Kimi Code usage with the CLI's fresh access token
// and the same device identity headers the official CLI sends.
type kimiCLIStrategy struct {
	accessToken     string
	identityHeaders map[string]string
	baseURL         string
	client          *http.Client
}

func (s *kimiCLIStrategy) Source() string { return "cli" }

func (s *kimiCLIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	data, err := kimiPerformGET(ctx, s.baseURL, s.accessToken, s.identityHeaders, s.client)
	if err != nil {
		return nil, err
	}
	return kimiSnapshotFromUsageResponse(data, s.Source())
}

// kimiWebStrategy fetches Kimi Code usage from the web billing endpoint
// with a manually supplied kimi-auth cookie JWT.
type kimiWebStrategy struct {
	authToken string
	client    *http.Client
}

func (s *kimiWebStrategy) Source() string { return "web" }

func (s *kimiWebStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, kimiWebUsagesURL, nil)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	req.Header.Set("Authorization", "Bearer "+s.authToken)
	req.Header.Set("Accept", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: kimiProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &kimiError{kind: "status", status: resp.StatusCode}
	}
	data, err := kimiReadAll(resp)
	if err != nil {
		return nil, err
	}
	return kimiSnapshotFromWebResponse(data, s.Source())
}

// MARK: - Parsing

func kimiSnapshotFromUsageResponse(data []byte, source string) (*UsageSnapshot, error) {
	var payload kimiUsageResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &kimiError{kind: "unparseable"}
	}
	meters := kimiMeters(payload.Usage, "Weekly quota")
	for _, limit := range payload.Limits {
		meters = append(meters, kimiMeters(limit.Detail, kimiWindowLabel(limit.Window))...)
	}
	return &UsageSnapshot{ProviderID: kimiProviderID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

func kimiSnapshotFromWebResponse(data []byte, source string) (*UsageSnapshot, error) {
	var payload kimiWebUsagesResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &kimiError{kind: "unparseable"}
	}
	var meters []UsageMeter
	var coding *kimiWebUsage
	for i := range payload.Usages {
		if payload.Usages[i].Scope == "FEATURE_CODING" {
			coding = &payload.Usages[i]
			break
		}
	}
	if coding == nil && len(payload.Usages) > 0 {
		coding = &payload.Usages[0]
	}
	if coding != nil {
		meters = append(meters, kimiMeters(coding.Detail, "Weekly quota")...)
		for _, limit := range coding.Limits {
			meters = append(meters, kimiMeters(limit.Detail, kimiWindowLabel(limit.Window))...)
		}
	}
	return &UsageSnapshot{ProviderID: kimiProviderID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// kimiMeters produces one meter per usage detail (used/limit/reset), or
// none when the payload carries no usable numbers.
func kimiMeters(detail *kimiUsageDetail, label string) []UsageMeter {
	if detail == nil {
		return nil
	}
	used, usedOK := parseOptionalFloat(detail.Used)
	limit, limitOK := parseOptionalFloat(detail.Limit)
	if !usedOK || !limitOK || limit <= 0 {
		return nil
	}
	var resetsAt *time.Time
	if detail.ResetTime != nil {
		resetsAt = kimiParseResetTime(*detail.ResetTime)
	}
	return []UsageMeter{{
		Label:    label,
		Used:     strPtr(formatAmount(used)),
		Limit:    strPtr(formatAmount(limit)),
		Unit:     "requests",
		ResetsAt: resetsAt,
	}}
}

// kimiWindowLabel produces a human label for a rate-limit window, e.g. "5h
// window" for the 300-minute window; falls back to a generic label.
func kimiWindowLabel(window *kimiWindow) string {
	if window == nil || window.Duration == nil || *window.Duration <= 0 {
		return "Rate limit window"
	}
	duration := *window.Duration
	if duration%60 == 0 {
		return strconv.Itoa(duration/60) + "h window"
	}
	return strconv.Itoa(duration) + "min window"
}

// kimiParseResetTime parses Kimi reset timestamps: ISO8601 with optional
// nanosecond fractional seconds, falling back to plain ISO8601.
func kimiParseResetTime(value string) *time.Time {
	if t, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return &t
	}
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return &t
	}
	return nil
}

// Kimi returns quota numbers as decimal strings ("2048") and reset times
// with nanosecond fractional seconds.
type kimiUsageDetail struct {
	Limit     *string `json:"limit"`
	Used      *string `json:"used"`
	Remaining *string `json:"remaining"`
	ResetTime *string `json:"resetTime"`
}

type kimiWindow struct {
	Duration *int    `json:"duration"`
	TimeUnit *string `json:"timeUnit"`
}

type kimiLimit struct {
	Window *kimiWindow      `json:"window"`
	Detail *kimiUsageDetail `json:"detail"`
}

// kimiUsageResponse — Kimi Code API response (GET /coding/v1/usages).
type kimiUsageResponse struct {
	Usage  *kimiUsageDetail `json:"usage"`
	Limits []kimiLimit      `json:"limits"`
}

// kimiWebUsagesResponse — web billing response (GetUsages).
type kimiWebUsagesResponse struct {
	Usages []kimiWebUsage `json:"usages"`
}

type kimiWebUsage struct {
	Scope  string           `json:"scope"`
	Detail *kimiUsageDetail `json:"detail"`
	Limits []kimiLimit      `json:"limits"`
}
