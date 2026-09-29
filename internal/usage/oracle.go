package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

// OracleFixture builds the source-owned provider graph from the production
// Provider and Strategy implementations. It is consumed by the checked-in
// oracle generator; credentials and sensitive headers are omitted from the
// emitted request view.
type OracleFixture struct {
	SchemaVersion int              `json:"schema_version"`
	Providers     []OracleProvider `json:"providers"`
}

type OracleProvider struct {
	ID       string         `json:"id"`
	Strategy string         `json:"strategy"`
	Fallback []string       `json:"fallback"`
	Auth     OracleAuth     `json:"auth"`
	Request  OracleRequest  `json:"request"`
	Response any            `json:"response"`
	Snapshot *UsageSnapshot `json:"snapshot"`
	Errors   []OracleError  `json:"errors"`
}

type OracleAuth struct {
	Configured bool   `json:"configured"`
	Source     string `json:"source"`
	Status     string `json:"status"`
}

// BuildClaudeFileTokenOracle reads a synthetic Claude credentials file with
// the shipped parser. It isolates HOME and USERPROFILE and never constructs a
// ClaudeProvider, so this parser oracle cannot consult the real keychain.
func BuildClaudeFileTokenOracle(contents []byte) (string, error) {
	home, err := os.MkdirTemp("", "symbrain-claude-file-oracle-home-")
	if err != nil {
		return "", fmt.Errorf("create isolated Claude oracle home: %w", err)
	}
	defer os.RemoveAll(home)

	previous := make(map[string]struct {
		value string
		set   bool
	}, 2)
	for _, name := range []string{"HOME", "USERPROFILE"} {
		value, set := os.LookupEnv(name)
		previous[name] = struct {
			value string
			set   bool
		}{value: value, set: set}
	}
	defer func() {
		for name, old := range previous {
			if old.set {
				_ = os.Setenv(name, old.value)
			} else {
				_ = os.Unsetenv(name)
			}
		}
	}()
	for _, name := range []string{"HOME", "USERPROFILE"} {
		if err := os.Setenv(name, home); err != nil {
			return "", fmt.Errorf("set isolated Claude oracle %s: %w", name, err)
		}
	}
	path := filepath.Join(home, ".claude", ".credentials.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return "", fmt.Errorf("create isolated Claude credential directory: %w", err)
	}
	if err := os.WriteFile(path, contents, 0o600); err != nil {
		return "", fmt.Errorf("write synthetic Claude credential file: %w", err)
	}
	return readClaudeFileToken(), nil
}

// BuildCopilotFileTokenOracle reads synthetic apps.json and hosts.json files
// with the shipped parser. A nil file argument means that file is absent.
func BuildCopilotFileTokenOracle(apps, hosts *string) (string, error) {
	dir, err := os.MkdirTemp("", "symbrain-copilot-file-oracle-")
	if err != nil {
		return "", fmt.Errorf("create isolated Copilot oracle directory: %w", err)
	}
	defer os.RemoveAll(dir)
	for _, file := range []struct {
		name    string
		content *string
	}{
		{name: "apps.json", content: apps},
		{name: "hosts.json", content: hosts},
	} {
		if file.content == nil {
			continue
		}
		path := filepath.Join(dir, file.name)
		if err := os.WriteFile(path, []byte(*file.content), 0o600); err != nil {
			return "", fmt.Errorf("write synthetic Copilot %s: %w", file.name, err)
		}
	}
	return readCopilotToken(dir), nil
}

// BuildNousFileTokenOracle reads a synthetic Hermes auth.json with the shipped
// Nous parser. A nil file argument means auth.json is absent.
func BuildNousFileTokenOracle(contents *string) (string, error) {
	dir, err := os.MkdirTemp("", "symbrain-nous-file-oracle-")
	if err != nil {
		return "", fmt.Errorf("create isolated Nous oracle directory: %w", err)
	}
	defer os.RemoveAll(dir)
	if contents != nil {
		path := filepath.Join(dir, "auth.json")
		if err := os.WriteFile(path, []byte(*contents), 0o600); err != nil {
			return "", fmt.Errorf("write synthetic Nous auth.json: %w", err)
		}
		return readNousAccessToken(path), nil
	}
	return readNousAccessToken(filepath.Join(dir, "auth.json")), nil
}

type OracleRequest struct {
	Method  string            `json:"method"`
	URL     string            `json:"url"`
	Headers map[string]string `json:"headers"`
	Body    string            `json:"body,omitempty"`
}

type OracleError struct {
	Status int    `json:"status,omitempty"`
	Kind   string `json:"kind"`
	Text   string `json:"text"`
}

type oracleProvider struct {
	id, name   string
	strategies []Strategy
}

func (p oracleProvider) ID() string             { return p.id }
func (p oracleProvider) DisplayName() string    { return p.name }
func (p oracleProvider) IsConfigured() bool     { return true }
func (p oracleProvider) Strategies() []Strategy { return p.strategies }
func (p oracleProvider) AuthStatus() AuthStatus {
	return AuthStatus{Status: "available", Detail: "fixture", Source: "fixture"}
}

type oracleProbe struct{ running bool }

func (p oracleProbe) processList() (string, bool) {
	if p.running {
		return "123 /opt/antigravity/language_server --app_data_dir antigravity --csrf_token fixture\n", true
	}
	return "", true
}
func (p oracleProbe) listeningPorts(int) (string, bool) {
	return "COMMAND PID NAME\nlanguage 123 TCP 127.0.0.1:43123 (LISTEN)\n", true
}
func (p oracleProbe) isAntigravityRunning() bool { return p.running }

type oracleTransport struct {
	bodies    map[string][]byte
	sequences map[string][][]byte
	status    int
	requests  []*http.Request
}

func (t *oracleTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, req.Clone(req.Context()))
	id := ""
	switch {
	case strings.Contains(req.URL.Host, "anthropic"):
		id = "claude"
	case strings.Contains(req.URL.Host, "chatgpt"):
		id = "codex"
	case strings.Contains(req.URL.Host, "github"):
		id = "copilot"
	case strings.Contains(req.URL.Host, "cursor"):
		id = "cursor"
	case strings.Contains(req.URL.Host, "kimi.com"):
		id = "kimi"
	case strings.Contains(req.URL.Host, "moonshot"):
		id = "moonshot"
	case strings.Contains(req.URL.Host, "nousresearch"):
		id = "nous"
	case strings.Contains(req.URL.Host, "opencode.ai"):
		id = "opencode"
	case strings.Contains(req.URL.Host, "openrouter"):
		id = "openrouter"
	case req.URL.Hostname() == "127.0.0.1":
		id = "antigravity"
	}
	body := t.bodyFor(id)
	status := t.status
	if status == 0 {
		status = http.StatusOK
	}
	return &http.Response{StatusCode: status, Body: http.NoBody, Header: make(http.Header), Request: req, ContentLength: int64(len(body))}, nil
}

func (t *oracleTransport) bodyFor(id string) []byte {
	sequence := t.sequences[id]
	if len(sequence) == 0 {
		return t.bodies[id]
	}
	seen := 0
	for _, request := range t.requests {
		if oracleID(request) == id {
			seen++
		}
	}
	index := seen - 1
	if index >= len(sequence) {
		index = len(sequence) - 1
	}
	return sequence[index]
}

// oracleTransportBody is used because http.NoBody cannot carry fixture bytes.
func (t *oracleTransport) response(req *http.Request, body []byte) *http.Response {
	status := t.status
	if status == 0 {
		status = http.StatusOK
	}
	return &http.Response{StatusCode: status, Body: ioNopCloser{Reader: bytes.NewReader(body)}, Header: make(http.Header), Request: req, ContentLength: int64(len(body))}
}

type ioNopCloser struct{ *bytes.Reader }

func (ioNopCloser) Close() error { return nil }

// BuildOracleFixture runs the real providers against deterministic fixture
// transport. The fixture directory is normally internal/usage/testdata.
func BuildOracleFixture(fixture map[string][]byte) (OracleFixture, error) {
	transport := &oracleTransport{bodies: fixture}
	client := &http.Client{Transport: roundTripFixture{transport}}
	providers := []Provider{
		oracleProvider{"claude", "Claude", []Strategy{&claudeOAuthStrategy{accessToken: "fixture", client: client}}},
		oracleProvider{"codex", "Codex", []Strategy{&codexOAuthStrategy{accessToken: "fixture", client: client}}},
		oracleProvider{"copilot", "GitHub Copilot", []Strategy{&copilotAPIStrategy{accessToken: "fixture", host: "github.com", client: client}}},
		oracleProvider{"cursor", "Cursor", []Strategy{&cursorWebStrategy{cookieHeader: "fixture", client: client}}},
		oracleProvider{"kimi", "Kimi Code", []Strategy{&kimiAPIStrategy{apiKey: "fixture", baseURL: kimiDefaultAPIBase, client: client}}},
		oracleProvider{"moonshot", "Moonshot", []Strategy{&moonshotAPIStrategy{apiKey: "fixture", region: MoonshotRegionInternational, client: client}}},
		oracleProvider{"nous", "Nous Portal", []Strategy{&nousPortalAPIStrategy{accessToken: "fixture", portalBaseURL: nousDefaultPortalURL, client: client}}},
		oracleProvider{"opencode", "OpenCode Go", []Strategy{&openCodeWebStrategy{cookieHeader: "fixture", workspaceOverride: "wrk_fixture", client: client}}},
		oracleProvider{"openrouter", "OpenRouter", []Strategy{&openRouterAPIStrategy{apiKey: "fixture", baseURL: openRouterDefaultBase, client: client}}},
		oracleProvider{"antigravity", "Antigravity", []Strategy{&antigravityLocalProbeStrategy{probe: oracleProbe{running: true}, client: client}}},
	}
	out := OracleFixture{SchemaVersion: ReportSchemaVersion}
	for _, provider := range providers {
		strategies := provider.Strategies()
		snapshot, err := RunStrategyChain(context.Background(), strategies)
		if err != nil {
			return OracleFixture{}, fmt.Errorf("oracle %s: %w", provider.ID(), err)
		}
		request := transport.requests[len(transport.requests)-1]
		transport.requests = nil
		out.Providers = append(out.Providers, OracleProvider{ID: provider.ID(), Strategy: snapshot.Source, Fallback: strategySources(strategies), Auth: OracleAuth{true, "fixture", "available"}, Request: safeOracleRequest(request), Response: fixtureValue(fixture[provider.ID()]), Snapshot: canonicalOracleSnapshot(snapshot), Errors: oracleErrors(provider, fixture)})
	}
	return out, nil
}

// BuildCopilotAuthenticatedReportOracle runs the shipped Copilot provider
// through BuildReport using the caller's synthetic COPILOT_ACCESS_TOKEN and
// a canned response body. The caller must set only a dummy token; this helper
// rejects missing or non-environment credential sources.
func BuildCopilotAuthenticatedReportOracle(body []byte) (Report, error) {
	return buildAuthenticatedDirectEnvReportOracle("copilot", body)
}

// BuildCopilotFileAuthenticatedReportOracle runs the shipped Copilot provider
// through BuildReport using only its default apps.json/hosts.json file and a
// canned response. The caller supplies an isolated HOME and synthetic file;
// no real credential or live provider is consulted.
func BuildCopilotFileAuthenticatedReportOracle(body []byte, status int) (Report, error) {
	if os.Getenv("COPILOT_ACCESS_TOKEN") != "" {
		return Report{}, fmt.Errorf("Copilot file report oracle requires only the default file credential")
	}
	transport := &oracleTransport{bodies: map[string][]byte{"copilot": body}, status: status}
	client := &http.Client{Transport: roundTripFixture{transport}}
	copilot := NewCopilotProvider(client)
	if !copilot.IsConfigured() || copilot.credSource != "file" {
		return Report{}, fmt.Errorf("Copilot file report oracle requires a file-sourced apps.json/hosts.json token")
	}
	if len(copilot.Strategies()) != 1 || copilot.Strategies()[0].Source() != "api" {
		return Report{}, fmt.Errorf("Copilot file report oracle requires exactly one API strategy")
	}
	report := BuildReport(context.Background(), []Provider{copilot})
	if len(report.Providers) != 1 || len(transport.requests) != 1 {
		return Report{}, fmt.Errorf("Copilot file report oracle did not produce one authenticated request")
	}
	request := transport.requests[0]
	if request.Method != http.MethodGet || request.URL.String() != "https://api.github.com/copilot_internal/user" ||
		request.Header.Get("Authorization") != "Bearer "+copilot.accessToken {
		return Report{}, fmt.Errorf("Copilot file report oracle did not issue the expected authenticated usage request")
	}
	if snapshot := report.Providers[0].Snapshot; snapshot != nil {
		snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	}
	return report, nil
}

// BuildClaudeAdminAuthenticatedReportOracle runs the shipped Claude provider
// through BuildReport with a direct ANTHROPIC_ADMIN_KEY and no OAuth source.
func BuildClaudeAdminAuthenticatedReportOracle(body []byte) (Report, error) {
	return buildAuthenticatedDirectEnvReportOracle("claude-admin", body)
}

// BuildCodexAuthenticatedReportOracle runs the shipped Codex provider through
// BuildReport with a direct CODEX_ACCESS_TOKEN and its default credential home.
func BuildCodexAuthenticatedReportOracle(body []byte) (Report, error) {
	if os.Getenv("CODEX_HOME") != "" {
		return Report{}, fmt.Errorf("Codex report oracle requires the default isolated credential home")
	}
	return buildAuthenticatedDirectEnvReportOracle("codex", body)
}

// BuildCodexFileAuthenticatedReportOracle runs the shipped Codex provider
// through BuildReport using only its read-only auth.json credential and a
// canned response. The caller supplies an isolated HOME containing
// ~/.codex/auth.json and a status/body pair; no Keychain or live provider is
// consulted.
func BuildCodexFileAuthenticatedReportOracle(body []byte, status int) (Report, error) {
	if os.Getenv("CODEX_HOME") != "" || os.Getenv("CODEX_ACCESS_TOKEN") != "" {
		return Report{}, fmt.Errorf("Codex file report oracle requires only the default auth.json credential")
	}
	transport := &oracleTransport{bodies: map[string][]byte{"codex": body}, status: status}
	client := &http.Client{Transport: roundTripFixture{transport}}
	providers := allProviders(client, func() (string, *time.Time) { return "", nil }, oracleProbe{})
	if len(providers) != 10 || providers[1].ID() != "codex" {
		return Report{}, fmt.Errorf("Codex file report oracle found an unexpected provider registry")
	}
	codex, ok := providers[1].(*CodexProvider)
	if !ok || !codex.IsConfigured() || codex.AuthStatus().Source != "file" {
		return Report{}, fmt.Errorf("Codex file report oracle requires a file-sourced auth.json token")
	}
	if len(codex.Strategies()) != 1 || codex.Strategies()[0].Source() != "oauth" {
		return Report{}, fmt.Errorf("Codex file report oracle requires exactly one OAuth strategy")
	}
	for index, provider := range providers {
		if index != 1 && index != 9 && provider.IsConfigured() {
			return Report{}, fmt.Errorf("Codex file report oracle found unexpected configured provider %q", provider.ID())
		}
	}
	if providers[9].AuthStatus().Source != "" {
		return Report{}, fmt.Errorf("Codex file report oracle Antigravity probe must be isolated and stopped")
	}
	report := BuildReport(context.Background(), providers)
	if len(report.Providers) != 10 || len(transport.requests) != 1 {
		return Report{}, fmt.Errorf("Codex file report oracle produced %d providers and %d requests, want 10 and one", len(report.Providers), len(transport.requests))
	}
	request := transport.requests[0]
	if request.Method != http.MethodGet || request.URL.String() != "https://chatgpt.com/backend-api/wham/usage" ||
		request.Header.Get("Authorization") != "Bearer "+codex.accessToken {
		return Report{}, fmt.Errorf("Codex file report oracle did not issue the expected authenticated usage request")
	}
	if snapshot := report.Providers[1].Snapshot; snapshot != nil {
		snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	}
	return report, nil
}

// BuildOpenRouterAuthenticatedReportOracle runs the shipped OpenRouter
// provider through BuildReport with a direct OPENROUTER_API_KEY and the
// default API base. The caller supplies only a synthetic key and canned body.
func BuildOpenRouterAuthenticatedReportOracle(body []byte) (Report, error) {
	if os.Getenv("OPENROUTER_API_URL") != "" {
		return Report{}, fmt.Errorf("OpenRouter report oracle requires the default API base")
	}
	return buildAuthenticatedDirectEnvReportOracle("openrouter", body)
}

// BuildMoonshotAuthenticatedReportOracle runs the shipped Moonshot provider
// through BuildReport with a direct MOONSHOT_API_KEY and the default `ai`
// region. The caller supplies only a synthetic key and canned body.
func BuildMoonshotAuthenticatedReportOracle(body []byte) (Report, error) {
	if os.Getenv("MOONSHOT_REGION") != "" {
		return Report{}, fmt.Errorf("Moonshot report oracle requires the default ai region")
	}
	return buildAuthenticatedDirectEnvReportOracle("moonshot", body)
}

// BuildCursorAuthenticatedReportOracle runs the shipped Cursor provider
// through BuildReport with a direct CURSOR_COOKIE and canned response body.
func BuildCursorAuthenticatedReportOracle(body []byte) (Report, error) {
	return buildAuthenticatedDirectEnvReportOracle("cursor", body)
}

// BuildKimiAuthenticatedReportOracle runs the shipped Kimi provider through
// BuildReport with a direct KIMI_CODE_API_KEY and the default API base.
func BuildKimiAuthenticatedReportOracle(body []byte) (Report, error) {
	if os.Getenv("KIMI_CODE_BASE_URL") != "" || os.Getenv("KIMI_CODE_HOME") != "" || os.Getenv("KIMI_AUTH_TOKEN") != "" {
		return Report{}, fmt.Errorf("Kimi report oracle requires only the direct API key, default base, and isolated credential home")
	}
	return buildAuthenticatedDirectEnvReportOracle("kimi", body)
}

// BuildNousAuthenticatedReportOracle runs the shipped Nous provider through
// BuildReport with a direct NOUS_PORTAL_ACCESS_TOKEN and default portal base.
func BuildNousAuthenticatedReportOracle(body []byte) (Report, error) {
	if os.Getenv("HERMES_PORTAL_BASE_URL") != "" || os.Getenv("HERMES_HOME") != "" {
		return Report{}, fmt.Errorf("Nous report oracle requires the default portal base and isolated credential home")
	}
	return buildAuthenticatedDirectEnvReportOracle("nous", body)
}

// BuildOpenCodeAuthenticatedReportOracle runs the shipped OpenCode provider
// through the ten-provider report with only a direct synthetic cookie. The
// fixture transport supplies workspace discovery and subscription responses.
func BuildOpenCodeAuthenticatedReportOracle(workspaceBody, subscriptionBody []byte) (Report, error) {
	if os.Getenv("OPENCODE_WORKSPACE_ID") != "" {
		return Report{}, fmt.Errorf("OpenCode report oracle requires workspace discovery without an override")
	}
	transport := &oracleTransport{
		sequences: map[string][][]byte{"opencode": {workspaceBody, subscriptionBody}},
	}
	client := &http.Client{Transport: roundTripFixture{transport}}
	providers := allProviders(client, func() (string, *time.Time) { return "", nil }, oracleProbe{})
	if len(providers) != 10 || providers[7].ID() != "opencode" || !providers[7].IsConfigured() || providers[7].AuthStatus().Source != "env" {
		return Report{}, fmt.Errorf("OpenCode report oracle requires only direct OPENCODE_COOKIE credentials")
	}
	for i, provider := range providers {
		if i != 7 && i != 9 && provider.IsConfigured() {
			return Report{}, fmt.Errorf("OpenCode report oracle found unexpected configured provider %q", provider.ID())
		}
	}
	if providers[9].AuthStatus().Source != "" {
		return Report{}, fmt.Errorf("OpenCode report oracle Antigravity probe must be isolated and stopped")
	}
	report := BuildReport(context.Background(), providers)
	if len(report.Providers) != 10 || report.Providers[7].Snapshot == nil {
		return Report{}, fmt.Errorf("OpenCode report oracle did not produce a snapshot")
	}
	if len(transport.requests) != 2 {
		return Report{}, fmt.Errorf("OpenCode report oracle made %d fixture requests, want workspace discovery and subscription", len(transport.requests))
	}
	if transport.requests[0].Method != http.MethodGet || transport.requests[0].URL.Query().Get("id") != openCodeWorkspacesServerID {
		return Report{}, fmt.Errorf("OpenCode report oracle did not discover the workspace using the shipped GET")
	}
	if transport.requests[1].Method != http.MethodGet || transport.requests[1].URL.Query().Get("id") != openCodeSubscriptionServerID {
		return Report{}, fmt.Errorf("OpenCode report oracle did not fetch the subscription using the shipped GET")
	}
	report.Providers[7].Snapshot = canonicalOracleSnapshot(report.Providers[7].Snapshot)
	return report, nil
}

func buildAuthenticatedDirectEnvReportOracle(providerID string, body []byte) (Report, error) {
	transportID := providerID
	if providerID == "claude-admin" {
		transportID = "claude"
	}
	transport := &oracleTransport{bodies: map[string][]byte{transportID: body}}
	client := &http.Client{Transport: roundTripFixture{transport}}
	providers := allProviders(client, func() (string, *time.Time) { return "", nil }, oracleProbe{})
	if len(providers) != 10 {
		return Report{}, fmt.Errorf("usage report oracle registered %d providers, want 10", len(providers))
	}
	index := map[string]int{"claude-admin": 0, "codex": 1, "copilot": 2, "cursor": 3, "kimi": 4, "moonshot": 5, "nous": 6, "openrouter": 8}[providerID]
	wantID := providerID
	if providerID == "claude-admin" {
		wantID = "claude"
	}
	if providers[index].ID() != wantID || !providers[index].IsConfigured() || providers[index].AuthStatus().Source != "env" {
		return Report{}, fmt.Errorf("%s report oracle requires its direct environment credential", providerID)
	}
	if providerID == "claude-admin" || providerID == "codex" || providerID == "kimi" || providerID == "nous" {
		strategies := providers[index].Strategies()
		wantSource := "api"
		if providerID == "codex" {
			wantSource = "oauth"
		}
		if len(strategies) != 1 || strategies[0].Source() != wantSource {
			return Report{}, fmt.Errorf("%s report oracle requires exactly one direct %s strategy", providerID, wantSource)
		}
	}
	for i, provider := range providers {
		if i != index && i != 9 && provider.IsConfigured() {
			return Report{}, fmt.Errorf("usage report oracle found unexpected configured provider %q", provider.ID())
		}
	}
	if providers[9].AuthStatus().Source != "" {
		return Report{}, fmt.Errorf("usage report oracle Antigravity probe must be isolated and stopped")
	}
	report := BuildReport(context.Background(), providers)
	if len(report.Providers) != 10 || report.Providers[index].Snapshot == nil {
		return Report{}, fmt.Errorf("%s report oracle did not produce a snapshot", providerID)
	}
	if len(transport.requests) != 1 {
		return Report{}, fmt.Errorf("usage report oracle made %d fixture requests, want only %s", len(transport.requests), providerID)
	}
	report.Providers[index].Snapshot = canonicalOracleSnapshot(report.Providers[index].Snapshot)
	return report, nil
}

type roundTripFixture struct{ target *oracleTransport }

func (r roundTripFixture) RoundTrip(req *http.Request) (*http.Response, error) {
	r.target.requests = append(r.target.requests, req.Clone(req.Context()))
	body := r.target.bodyFor(oracleID(req))
	return r.target.response(req, body), nil
}
func oracleID(req *http.Request) string {
	h := req.URL.Host
	switch {
	case strings.Contains(h, "anthropic"):
		return "claude"
	case strings.Contains(h, "chatgpt"):
		return "codex"
	case strings.Contains(h, "github"):
		return "copilot"
	case strings.Contains(h, "cursor"):
		return "cursor"
	case strings.Contains(h, "kimi.com"):
		return "kimi"
	case strings.Contains(h, "moonshot"):
		return "moonshot"
	case strings.Contains(h, "nousresearch"):
		return "nous"
	case strings.Contains(h, "opencode.ai"):
		return "opencode"
	case strings.Contains(h, "openrouter"):
		return "openrouter"
	default:
		return "antigravity"
	}
}
func strategySources(strategies []Strategy) []string {
	out := make([]string, 0, len(strategies))
	for _, s := range strategies {
		out = append(out, s.Source())
	}
	return out
}
func safeOracleRequest(req *http.Request) OracleRequest {
	h := map[string]string{}
	for _, n := range []string{"Accept", "Content-Type", "Connect-Protocol-Version", "X-Title", "X-Server-Id", "anthropic-beta"} {
		if v := req.Header.Get(n); v != "" {
			h[n] = v
		}
	}
	body := ""
	if req.Body != nil {
		body = "{}"
	}
	u := req.URL.String()
	u = regexp.MustCompile(`127\.0\.0\.1:[0-9]+`).ReplaceAllString(u, "127.0.0.1:<port>")
	return OracleRequest{Method: req.Method, URL: u, Headers: h, Body: body}
}
func canonicalOracleSnapshot(s *UsageSnapshot) *UsageSnapshot {
	c := *s
	fixed := time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	c.FetchedAt = fixed
	for i := range c.Meters {
		meter := &c.Meters[i]
		if meter.ResetsAt == nil {
			continue
		}
		switch meter.Label {
		case "5h window":
			t := fixed.Add(time.Hour)
			meter.ResetsAt = &t
		case "This week":
			t := fixed.Add(24 * time.Hour)
			meter.ResetsAt = &t
		}
	}
	return &c
}
func fixtureValue(data []byte) any {
	var v any
	if json.Unmarshal(data, &v) == nil {
		return v
	}
	return string(data)
}
func oracleErrors(provider Provider, fixture map[string][]byte) []OracleError {
	out := []OracleError{}
	for _, status := range []int{401, 429} {
		rt := &statusTransport{status: status, body: fixture[provider.ID()]}
		p := oracleWithTransport(provider, rt)
		_, err := RunStrategyChain(context.Background(), p.Strategies())
		out = append(out, oracleError(status, err))
	}
	rt := &statusTransport{status: 200, body: []byte(`{}`)}
	p := oracleWithTransport(provider, rt)
	_, err := RunStrategyChain(context.Background(), p.Strategies())
	out = append(out, oracleError(0, err))
	return out
}

type statusTransport struct {
	status int
	body   []byte
}

func (t *statusTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	return &http.Response{StatusCode: t.status, Body: ioNopCloser{Reader: bytes.NewReader(t.body)}, Header: http.Header{"Retry-After": []string{"17"}}, Request: req}, nil
}
func oracleWithTransport(p Provider, rt http.RoundTripper) Provider {
	c := &http.Client{Transport: rt}
	switch p.ID() {
	case "claude":
		return oracleProvider{"claude", "Claude", []Strategy{&claudeOAuthStrategy{accessToken: "fixture", client: c}}}
	case "codex":
		return oracleProvider{"codex", "Codex", []Strategy{&codexOAuthStrategy{accessToken: "fixture", client: c}}}
	case "copilot":
		return oracleProvider{"copilot", "GitHub Copilot", []Strategy{&copilotAPIStrategy{accessToken: "fixture", host: "github.com", client: c}}}
	case "cursor":
		return oracleProvider{"cursor", "Cursor", []Strategy{&cursorWebStrategy{cookieHeader: "fixture", client: c}}}
	case "kimi":
		return oracleProvider{"kimi", "Kimi Code", []Strategy{&kimiAPIStrategy{apiKey: "fixture", baseURL: kimiDefaultAPIBase, client: c}}}
	case "moonshot":
		return oracleProvider{"moonshot", "Moonshot", []Strategy{&moonshotAPIStrategy{apiKey: "fixture", region: MoonshotRegionInternational, client: c}}}
	case "nous":
		return oracleProvider{"nous", "Nous Portal", []Strategy{&nousPortalAPIStrategy{accessToken: "fixture", portalBaseURL: nousDefaultPortalURL, client: c}}}
	case "opencode":
		return oracleProvider{"opencode", "OpenCode Go", []Strategy{&openCodeWebStrategy{cookieHeader: "fixture", workspaceOverride: "wrk_fixture", client: c}}}
	case "openrouter":
		return oracleProvider{"openrouter", "OpenRouter", []Strategy{&openRouterAPIStrategy{apiKey: "fixture", baseURL: openRouterDefaultBase, client: c}}}
	default:
		return oracleProvider{"antigravity", "Antigravity", []Strategy{&antigravityLocalProbeStrategy{probe: oracleProbe{running: true}, client: c}}}
	}
}
func oracleError(status int, err error) OracleError {
	kind := "parse"
	if status == 401 {
		kind = "auth"
	} else if status == 429 {
		kind = "rate_limited"
	}
	return OracleError{Status: status, Kind: kind, Text: err.Error()}
}
