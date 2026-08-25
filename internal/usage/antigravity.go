package usage

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"os/exec"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const (
	antigravityProviderID  = "antigravity"
	antigravityDisplayName = "Antigravity"
	antigravityServicePath = "exa.language_server_pb.LanguageServerService"
)

// AntigravityProvider — Antigravity usage provider.
//
// Quota is read from the local language-server probe while the Antigravity
// app or `agy` CLI is running — Antigravity is never started by this
// provider, and nothing is scraped from any UI. When no local quota server
// is reachable the strategy reports the provider as not available, never a
// hard failure with fake numbers.
//
// Google Cloud Code OAuth is intentionally not implemented here, matching
// the Swift original: the local probe covers the documented acceptance
// criteria, and the OAuth path would need Google credentials this tool
// does not manage. Mirrors symaira-cockpit's AntigravityUsageProvider.
type AntigravityProvider struct {
	probe antigravityProcessProbe
}

// NewAntigravityProvider is always "configured" — whether a quota is
// available depends on the running language server, not on configuration.
func NewAntigravityProvider() *AntigravityProvider {
	return &AntigravityProvider{probe: shellProcessProbe{}}
}

func (p *AntigravityProvider) ID() string          { return antigravityProviderID }
func (p *AntigravityProvider) DisplayName() string { return antigravityDisplayName }
func (p *AntigravityProvider) IsConfigured() bool  { return true }

func (p *AntigravityProvider) Strategies() []Strategy {
	return []Strategy{&antigravityLocalProbeStrategy{probe: p.probe}}
}

func (p *AntigravityProvider) AuthStatus() AuthStatus {
	if p.probe.isAntigravityRunning() {
		return AuthStatus{Status: "available", Detail: "Antigravity is running", Source: "local"}
	}
	return AuthStatus{Status: "missing", Detail: "Antigravity is not running — start the Antigravity app or agy CLI"}
}

// MARK: - Process probe

type antigravityProcessProbe interface {
	processList() (string, bool)
	listeningPorts(pid int) (string, bool)
	isAntigravityRunning() bool
}

// shellProcessProbe is the production probe, backed by ps and lsof on
// PATH (portable lookup, unlike the Swift original's hardcoded macOS
// paths — symbrain also targets linux; on a platform without these tools
// the probe simply reports nothing, degrading to "not running").
type shellProcessProbe struct{}

func (shellProcessProbe) processList() (string, bool) {
	out, err := exec.Command("ps", "-ax", "-o", "pid=,command=").Output()
	if err != nil {
		return "", false
	}
	return string(out), true
}

func (shellProcessProbe) listeningPorts(pid int) (string, bool) {
	out, err := exec.Command("lsof", "-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", strconv.Itoa(pid)).Output()
	if err != nil {
		return "", false
	}
	return string(out), true
}

func (s shellProcessProbe) isAntigravityRunning() bool {
	list, ok := s.processList()
	if !ok {
		return false
	}
	return strings.Contains(list, "agy") || strings.Contains(list, "Antigravity")
}

// MARK: - Local transport

// antigravityLoopbackTransport trusts self-signed certificates only for
// loopback hosts (the Antigravity local language server serves one) —
// mirrors the Swift original's LoopbackTrustDelegate scoping.
type antigravityLoopbackTransport struct {
	insecure *http.Transport
	safe     *http.Transport
}

func newAntigravityLoopbackTransport() *antigravityLoopbackTransport {
	return &antigravityLoopbackTransport{
		insecure: &http.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}}, //nolint:gosec // scoped to loopback hosts only, see RoundTrip
		safe:     &http.Transport{},
	}
}

func (t *antigravityLoopbackTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	if antigravityIsLoopbackHost(req.URL.Hostname()) {
		return t.insecure.RoundTrip(req)
	}
	return t.safe.RoundTrip(req)
}

func antigravityIsLoopbackHost(host string) bool {
	return host == "127.0.0.1" || host == "localhost" || host == "::1"
}

func newAntigravityHTTPClient() *http.Client {
	return &http.Client{Transport: newAntigravityLoopbackTransport(), Timeout: 6 * time.Second}
}

// MARK: - Errors

type antigravityError struct {
	kind   string // not_running | probe_failed | http_status | parse_failed
	status int
	detail string
}

func (e *antigravityError) Error() string {
	switch e.kind {
	case "not_running":
		return "Antigravity is not running — no local quota server found."
	case "probe_failed":
		return fmt.Sprintf("Antigravity probe failed: %s", e.detail)
	case "http_status":
		return fmt.Sprintf("Antigravity local server returned HTTP %d.", e.status)
	default:
		return fmt.Sprintf("Antigravity returned an unreadable response: %s", e.detail)
	}
}

// MARK: - Strategy

// antigravityLocalProbeStrategy probes a running Antigravity language
// server for quota. Flow: ps -> language-server candidates (with
// --csrf_token when present) -> lsof listening ports per candidate ->
// GetUnleashData connect probe -> RetrieveUserQuotaSummary, falling back
// to GetUserStatus, then GetCommandModelConfigs.
type antigravityLocalProbeStrategy struct {
	probe  antigravityProcessProbe
	client *http.Client
}

func (s *antigravityLocalProbeStrategy) Source() string { return "local" }

func (s *antigravityLocalProbeStrategy) httpClient() *http.Client {
	if s.client != nil {
		return s.client
	}
	return newAntigravityHTTPClient()
}

func (s *antigravityLocalProbeStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	list, _ := s.probe.processList()
	candidates := antigravityParseCandidates(list)
	if len(candidates) == 0 {
		return nil, &antigravityError{kind: "not_running"}
	}

	var lastErr error = &antigravityError{kind: "not_running"}
	client := s.httpClient()
	for _, candidate := range candidates {
		portList, _ := s.probe.listeningPorts(candidate.pid)
		for _, port := range antigravityParsePorts(portList) {
			snap, err := antigravityFetchFromPort(ctx, client, port, candidate.csrfToken)
			if err == nil {
				return snap, nil
			}
			lastErr = err
		}
	}
	return nil, lastErr
}

// antigravityServerCandidate is one discovered language-server process.
type antigravityServerCandidate struct {
	pid       int
	csrfToken string
}

// antigravityParseCandidates extracts Antigravity language-server
// processes from ps output. Recognizes the app/IDE language_server*
// binaries (scoped to Antigravity by --app_data_dir antigravity or an
// antigravity path) and the agy CLI binary (which needs no CSRF token).
func antigravityParseCandidates(processList string) []antigravityServerCandidate {
	var candidates []antigravityServerCandidate
	for _, line := range strings.Split(processList, "\n") {
		trimmed := strings.TrimLeft(line, " ")
		idx := strings.IndexByte(trimmed, ' ')
		if idx < 0 {
			continue
		}
		pidStr := trimmed[:idx]
		pid, err := strconv.Atoi(pidStr)
		if err != nil {
			continue
		}
		command := strings.TrimLeft(trimmed[idx:], " ")
		lower := strings.ToLower(command)

		isAppOrIDEServer := (strings.Contains(lower, "language_server") || strings.Contains(lower, "language-server")) &&
			(strings.Contains(lower, "antigravity") ||
				(strings.Contains(lower, "--app_data_dir") && strings.Contains(command, "antigravity")))
		isCLI := strings.Contains(lower, "/agy") || strings.Contains(lower, "antigravity-cli") || strings.Contains(lower, "antigravity_cli")
		if !isAppOrIDEServer && !isCLI {
			continue
		}

		var csrfToken string
		if tokenIdx := strings.Index(command, "--csrf_token"); tokenIdx >= 0 {
			after := strings.TrimLeft(command[tokenIdx+len("--csrf_token"):], " ")
			fields := strings.SplitN(after, " ", 2)
			if len(fields) > 0 && fields[0] != "" {
				csrfToken = fields[0]
			}
		}
		candidates = append(candidates, antigravityServerCandidate{pid: pid, csrfToken: csrfToken})
	}
	return candidates
}

var antigravityPortPattern = regexp.MustCompile(`:([0-9]{1,5})(?:\s|$)`)

// antigravityParsePorts extracts listening TCP ports from
// lsof -iTCP -sTCP:LISTEN output. The NAME column
// (TCP 127.0.0.1:34567 (LISTEN)) contains spaces, so the whole line is
// scanned for :port tokens.
func antigravityParsePorts(portList string) []int {
	var ports []int
	seen := map[int]bool{}
	for _, line := range strings.Split(portList, "\n") {
		if !strings.Contains(line, "(LISTEN)") {
			continue
		}
		match := antigravityPortPattern.FindStringSubmatch(line)
		if len(match) < 2 {
			continue
		}
		port, err := strconv.Atoi(match[1])
		if err != nil || seen[port] {
			continue
		}
		seen[port] = true
		ports = append(ports, port)
	}
	return ports
}

func antigravityFetchFromPort(ctx context.Context, client *http.Client, port int, csrfToken string) (*UsageSnapshot, error) {
	base := fmt.Sprintf("https://127.0.0.1:%d", port)

	connectReq, err := http.NewRequestWithContext(ctx, http.MethodPost, base+"/"+antigravityServicePath+"/GetUnleashData", nil)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: err.Error()}
	}
	if csrfToken != "" {
		connectReq.Header.Set("X-Codeium-Csrf-Token", csrfToken)
	}
	connectReq.Header.Set("Connect-Protocol-Version", "1")

	connectResp, err := client.Do(connectReq)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: fmt.Sprintf("connect probe failed on port %d", port)}
	}
	connectResp.Body.Close()
	if connectResp.StatusCode < 200 || connectResp.StatusCode >= 300 {
		return nil, &antigravityError{kind: "probe_failed", detail: fmt.Sprintf("connect probe failed on port %d", port)}
	}

	// Quota chain: RetrieveUserQuotaSummary -> GetUserStatus ->
	// GetCommandModelConfigs. Any failure (HTTP or parse) falls through to
	// the next endpoint; the last error is returned when all fail.
	var chainErr error = &antigravityError{kind: "parse_failed", detail: "all quota endpoints failed"}
	for _, method := range []string{"RetrieveUserQuotaSummary", "GetUserStatus", "GetCommandModelConfigs"} {
		data, err := antigravityPostJSON(ctx, client, base, method, csrfToken)
		if err != nil {
			chainErr = err
			continue
		}
		snap, err := antigravitySnapshot(method, data, antigravityProviderID, "local")
		if err == nil {
			return snap, nil
		}
		chainErr = &antigravityError{kind: "parse_failed", detail: method + " returned unreadable data"}
	}
	return nil, chainErr
}

func antigravityPostJSON(ctx context.Context, client *http.Client, base, method, csrfToken string) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, base+"/"+antigravityServicePath+"/"+method, bytes.NewReader([]byte("{}")))
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: err.Error()}
	}
	if csrfToken != "" {
		req.Header.Set("X-Codeium-Csrf-Token", csrfToken)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")

	resp, err := client.Do(req)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: err.Error()}
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: antigravityProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &antigravityError{kind: "http_status", status: resp.StatusCode}
	}
	return io.ReadAll(resp.Body)
}

// MARK: - Response parsing

// antigravitySnapshot dispatches by endpoint name (used by the strategy's
// fallback chain).
func antigravitySnapshot(method string, data []byte, providerID, source string) (*UsageSnapshot, error) {
	switch method {
	case "RetrieveUserQuotaSummary":
		return antigravityQuotaSummarySnapshot(data, providerID, source)
	case "GetUserStatus":
		return antigravityUserStatusSnapshot(data, providerID, source)
	default:
		return antigravityModelConfigsSnapshot(data, providerID, source)
	}
}

// antigravityQuotaSummarySnapshot parses RetrieveUserQuotaSummary: quota
// groups with named buckets (e.g. "Gemini Models" / "Claude and GPT
// models", each with weekly and five-hour buckets). Each bucket's
// remainingFraction maps to a percent meter.
func antigravityQuotaSummarySnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityQuotaSummaryEnvelope
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary is not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary rejected"}
	}
	summary := payload.Response
	if summary == nil {
		summary = payload.Summary
	}
	if summary == nil && len(payload.Groups) > 0 {
		summary = &antigravityQuotaSummaryPayload{Groups: payload.Groups}
	}
	if summary == nil || len(summary.Groups) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "missing quota groups"}
	}

	var meters []UsageMeter
	for _, group := range summary.Groups {
		for _, bucket := range group.Buckets {
			if bucket.RemainingFraction == nil || *bucket.RemainingFraction < 0 || *bucket.RemainingFraction > 1 {
				continue
			}
			label := strings.TrimSpace(bucket.displayLabel())
			if label == "" {
				label = group.DisplayName
			}
			var resetsAt *time.Time
			if bucket.ResetTime != nil {
				resetsAt = antigravityParseResetTime(*bucket.ResetTime)
			}
			meters = append(meters, UsageMeter{
				Label:    group.DisplayName + " — " + label,
				Used:     strPtr(formatAmount(antigravityRoundPercent(1 - *bucket.RemainingFraction))),
				Limit:    strPtr("100"),
				Unit:     "%",
				ResetsAt: resetsAt,
			})
		}
	}
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary has no usable buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// antigravityUserStatusSnapshot parses GetUserStatus: plan plus per-model
// quotaInfo buckets.
func antigravityUserStatusSnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityUserStatusResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status is not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status rejected"}
	}
	var configs []antigravityModelConfig
	if payload.UserStatus != nil && payload.UserStatus.CascadeModelConfigData != nil {
		configs = payload.UserStatus.CascadeModelConfigData.ClientModelConfigs
	}
	meters := antigravityModelConfigMeters(configs)
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status has no quota buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// antigravityModelConfigsSnapshot parses GetCommandModelConfigs: per-model
// quotaInfo buckets.
func antigravityModelConfigsSnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityModelConfigResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs are not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs rejected"}
	}
	meters := antigravityModelConfigMeters(payload.ClientModelConfigs)
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs have no quota buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

func antigravityModelConfigMeters(configs []antigravityModelConfig) []UsageMeter {
	var meters []UsageMeter
	for _, config := range configs {
		if config.QuotaInfo == nil || config.QuotaInfo.RemainingFraction == nil {
			continue
		}
		remaining := *config.QuotaInfo.RemainingFraction
		if remaining < 0 || remaining > 1 {
			continue
		}
		label := strings.TrimSpace(config.displayLabel())
		if label == "" {
			label = config.ModelOrAlias.Model
		}
		var resetsAt *time.Time
		if config.QuotaInfo.ResetTime != nil {
			resetsAt = antigravityParseResetTime(*config.QuotaInfo.ResetTime)
		}
		meters = append(meters, UsageMeter{
			Label:    label,
			Used:     strPtr(formatAmount(antigravityRoundPercent(1 - remaining))),
			Limit:    strPtr("100"),
			Unit:     "%",
			ResetsAt: resetsAt,
		})
	}
	return meters
}

// antigravityRoundPercent converts a 0..1 "used fraction" to a whole
// percent — the raw fraction math carries binary floating-point noise
// (58.000...1), matching the Swift original's explicit .rounded().
func antigravityRoundPercent(usedFraction float64) float64 {
	return math.Round(usedFraction * 100)
}

// antigravityParseResetTime parses Antigravity reset timestamps: ISO8601
// with optional fractional seconds, falling back to plain ISO8601.
func antigravityParseResetTime(value string) *time.Time {
	if t, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return &t
	}
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return &t
	}
	return nil
}

// MARK: - Response models

// antigravityCode arrives either as an integer (0 = ok) or a string
// ("ok").
type antigravityCode struct {
	intValue    *int
	stringValue *string
}

func (c *antigravityCode) UnmarshalJSON(data []byte) error {
	var i int
	if err := json.Unmarshal(data, &i); err == nil {
		c.intValue = &i
		return nil
	}
	var s string
	if err := json.Unmarshal(data, &s); err == nil {
		c.stringValue = &s
		return nil
	}
	empty := ""
	c.stringValue = &empty
	return nil
}

func (c antigravityCode) isOK() bool {
	if c.intValue != nil {
		return *c.intValue == 0
	}
	if c.stringValue != nil {
		lower := strings.ToLower(*c.stringValue)
		return lower == "ok" || lower == "success" || *c.stringValue == "0"
	}
	return false
}

type antigravityQuotaSummaryEnvelope struct {
	Code     antigravityCode                 `json:"code"`
	Message  *string                         `json:"message"`
	Response *antigravityQuotaSummaryPayload `json:"response"`
	Summary  *antigravityQuotaSummaryPayload `json:"summary"`
	Groups   []antigravityQuotaGroup         `json:"groups"`
}

type antigravityQuotaSummaryPayload struct {
	Description *string                 `json:"description"`
	Groups      []antigravityQuotaGroup `json:"groups"`
}

type antigravityQuotaGroup struct {
	DisplayName string                   `json:"displayName"`
	Description *string                  `json:"description"`
	Buckets     []antigravityQuotaBucket `json:"buckets"`
}

type antigravityQuotaBucket struct {
	BucketID          string   `json:"bucketId"`
	DisplayName       *string  `json:"displayName"`
	RemainingFraction *float64 `json:"remainingFraction"`
	ResetTime         *string  `json:"resetTime"`
	Description       *string  `json:"description"`
	Disabled          *bool    `json:"disabled"`
}

func (b antigravityQuotaBucket) displayLabel() string {
	if b.DisplayName == nil {
		return ""
	}
	return *b.DisplayName
}

type antigravityUserStatusResponse struct {
	Code       antigravityCode        `json:"code"`
	Message    *string                `json:"message"`
	UserStatus *antigravityUserStatus `json:"userStatus"`
}

type antigravityUserStatus struct {
	Email                  *string                     `json:"email"`
	CascadeModelConfigData *antigravityModelConfigData `json:"cascadeModelConfigData"`
}

type antigravityModelConfigData struct {
	ClientModelConfigs []antigravityModelConfig `json:"clientModelConfigs"`
}

type antigravityModelConfigResponse struct {
	Code               antigravityCode          `json:"code"`
	Message            *string                  `json:"message"`
	ClientModelConfigs []antigravityModelConfig `json:"clientModelConfigs"`
}

type antigravityModelConfig struct {
	Label        *string               `json:"label"`
	ModelOrAlias antigravityModelAlias `json:"modelOrAlias"`
	QuotaInfo    *antigravityQuotaInfo `json:"quotaInfo"`
}

func (c antigravityModelConfig) displayLabel() string {
	if c.Label == nil {
		return ""
	}
	return *c.Label
}

type antigravityModelAlias struct {
	Model string `json:"model"`
}

type antigravityQuotaInfo struct {
	RemainingFraction *float64 `json:"remainingFraction"`
	ResetTime         *string  `json:"resetTime"`
}
