package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
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
		client = newProviderHTTPClient()
	}
	cliHome := kimiCLIHome()
	store := kimiCLICredentialStore{home: cliHome}
	baseURL := validatedUsageBase(os.Getenv("KIMI_CODE_BASE_URL"), kimiDefaultAPIBase)
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
	data, err := readCredentialFile(filepath.Join(s.home, "credentials", "kimi-code.json"))
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
	data, err := readCredentialFile(filepath.Join(s.home, "device_id"))
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

	resp, err := doUsageRequest(ctx, client, req, false)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	data, err := kimiReadAll(resp)
	if err != nil {
		return nil, err
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: kimiProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &kimiError{kind: "status", status: resp.StatusCode}
	}
	return data, nil
}

func kimiReadAll(resp *http.Response) ([]byte, error) {
	data, err := readUsageBodyAndClose(resp.Body)
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

	resp, err := doUsageRequest(ctx, s.client, req, false)
	if err != nil {
		return nil, &kimiError{kind: "network", detail: err.Error()}
	}
	data, err := kimiReadAll(resp)
	if err != nil {
		return nil, err
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: kimiProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &kimiError{kind: "status", status: resp.StatusCode}
	}
	return kimiSnapshotFromWebResponse(data, s.Source())
}

// MARK: - Parsing
