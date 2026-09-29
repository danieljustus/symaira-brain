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
	"strconv"
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

// BuildKimiFileTokenOracle reads a synthetic Kimi CLI credential file with
// the shipped typed decoder. A nil file argument means the file is absent.
func BuildKimiFileTokenOracle(contents *string) (string, error) {
	dir, err := os.MkdirTemp("", "symbrain-kimi-file-oracle-")
	if err != nil {
		return "", fmt.Errorf("create isolated Kimi oracle directory: %w", err)
	}
	defer os.RemoveAll(dir)
	if contents == nil {
		return (kimiCLICredentialStore{home: dir}).readAccessToken(), nil
	}
	path := filepath.Join(dir, "credentials", "kimi-code.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return "", fmt.Errorf("create synthetic Kimi credentials directory: %w", err)
	}
	if err := os.WriteFile(path, []byte(*contents), 0o600); err != nil {
		return "", fmt.Errorf("write synthetic Kimi credentials: %w", err)
	}
	return (kimiCLICredentialStore{home: dir}).readAccessToken(), nil
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

type antigravityOracleProbe struct {
	processes  string
	portsByPID map[int]string
}

func (p antigravityOracleProbe) processList() (string, bool) { return p.processes, true }
func (p antigravityOracleProbe) listeningPorts(pid int) (string, bool) {
	ports, ok := p.portsByPID[pid]
	return ports, ok
}
func (p antigravityOracleProbe) isAntigravityRunning() bool {
	return strings.Contains(p.processes, "agy") || strings.Contains(p.processes, "Antigravity")
}

type antigravityOracleReply struct {
	status int
	body   []byte
	inputs map[string]string
}

type antigravityOracleTransport struct {
	replies   map[string]antigravityOracleReply
	requests  []*http.Request
	responses []AntigravityOracleResponse
}

type AntigravityOracleResponse struct {
	Status  int               `json:"status"`
	Body    string            `json:"body"`
	Headers map[string]string `json:"headers,omitempty"`
}

func (t *antigravityOracleTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, req.Clone(req.Context()))
	parts := strings.Split(strings.Trim(req.URL.Path, "/"), "/")
	if len(parts) == 0 {
		return nil, fmt.Errorf("Antigravity oracle received an empty path")
	}
	port, err := strconv.Atoi(req.URL.Port())
	if err != nil {
		return nil, fmt.Errorf("Antigravity oracle received invalid port")
	}
	reply, ok := t.replies[fmt.Sprintf("%d/%s", port, parts[len(parts)-1])]
	if !ok {
		return nil, fmt.Errorf("Antigravity oracle has no reply for port %d method %s", port, parts[len(parts)-1])
	}
	header := make(http.Header)
	for name, value := range reply.inputs {
		header.Set(name, value)
	}
	if reply.status == 0 {
		reply.status = http.StatusOK
	}
	response := AntigravityOracleResponse{Status: reply.status, Body: string(reply.body), Headers: reply.inputs}
	t.responses = append(t.responses, response)
	return &http.Response{
		StatusCode: reply.status,
		Body:       ioNopCloser{Reader: bytes.NewReader(reply.body)},
		Header:     header,
		Request:    req,
	}, nil
}

type AntigravityReportOracleCase struct {
	ID          string                      `json:"id"`
	ProcessList string                      `json:"process_list"`
	PortsByPID  map[int]string              `json:"ports_by_pid"`
	Responses   []AntigravityOracleResponse `json:"responses"`
	Report      Report                      `json:"report"`
	Requests    []OracleRequest             `json:"requests"`
}

type AntigravityReportOracleFixture struct {
	SchemaVersion int                           `json:"schema_version"`
	Cases         []AntigravityReportOracleCase `json:"cases"`
}

// BuildAntigravityAuthenticatedReportOracle runs the production provider and
// report assembly against synthetic process/port observations and local canned
// responses. It never starts Antigravity or contacts a live endpoint.
func BuildAntigravityAuthenticatedReportOracle(fixtures map[string][]byte) (AntigravityReportOracleFixture, error) {
	for _, name := range []string{"antigravity", "antigravity-user-status"} {
		if len(fixtures[name]) == 0 {
			return AntigravityReportOracleFixture{}, fmt.Errorf("Antigravity oracle fixture %q is empty", name)
		}
	}
	server := " 111 /Applications/Antigravity.app/Contents/Resources/language_server --app_data_dir antigravity --csrf_token oracle-csrf\n"
	fallbackServer := " 333 /opt/homebrew/bin/agy\n"
	failedServer := " 444 /Applications/Antigravity.app/Contents/Resources/language_server --app_data_dir antigravity --csrf_token oracle-csrf\n"
	cases := []struct {
		id           string
		processes    string
		ports        map[int]string
		replies      map[string]antigravityOracleReply
		wantSnapshot bool
		wantError    string
		wantRequests []string
	}{
		{
			id:           "active-second-port-after-connect-failure",
			processes:    server,
			ports:        map[int]string{111: "COMMAND PID NAME\nlanguage 111 TCP 127.0.0.1:43121 (LISTEN)\nlanguage 111 TCP 127.0.0.1:43122 (LISTEN)\n"},
			wantSnapshot: true,
			wantRequests: []string{"43121/GetUnleashData", "43122/GetUnleashData", "43122/RetrieveUserQuotaSummary"},
			replies: map[string]antigravityOracleReply{
				"43121/GetUnleashData":           {status: http.StatusServiceUnavailable},
				"43122/GetUnleashData":           {status: http.StatusOK},
				"43122/RetrieveUserQuotaSummary": {status: http.StatusOK, body: fixtures["antigravity"]},
			},
		},
		{
			id:           "fallback-to-user-status",
			processes:    fallbackServer,
			ports:        map[int]string{333: "agy 333 daniel 14u IPv4 0x123 0t0 TCP 127.0.0.1:43123 (LISTEN)\n"},
			wantSnapshot: true,
			wantRequests: []string{"43123/GetUnleashData", "43123/RetrieveUserQuotaSummary", "43123/GetUserStatus"},
			replies: map[string]antigravityOracleReply{
				"43123/GetUnleashData":           {status: http.StatusOK},
				"43123/RetrieveUserQuotaSummary": {status: http.StatusOK, body: []byte("not json")},
				"43123/GetUserStatus":            {status: http.StatusOK, body: fixtures["antigravity-user-status"]},
			},
		},
		{
			id:        "fallback-to-next-candidate",
			processes: " 555 /Applications/Antigravity.app/Contents/Resources/language_server --app_data_dir antigravity --csrf_token first-candidate\n 556 /opt/homebrew/bin/agy\n",
			ports: map[int]string{
				555: "language 555 TCP 127.0.0.1:43125 (LISTEN)\n",
				556: "agy 556 daniel 14u IPv4 0x123 0t0 TCP 127.0.0.1:43126 (LISTEN)\n",
			},
			wantSnapshot: true,
			wantRequests: []string{"43125/GetUnleashData", "43126/GetUnleashData", "43126/RetrieveUserQuotaSummary"},
			replies: map[string]antigravityOracleReply{
				"43125/GetUnleashData":           {status: http.StatusServiceUnavailable},
				"43126/GetUnleashData":           {status: http.StatusOK},
				"43126/RetrieveUserQuotaSummary": {status: http.StatusOK, body: fixtures["antigravity"]},
			},
		},
		{
			id:           "not-running-no-request",
			wantError:    "all AI usage fallbacks failed: Antigravity is not running — no local quota server found.",
			wantRequests: []string{},
		},
		{
			id:           "all-endpoints-fail",
			processes:    failedServer,
			ports:        map[int]string{444: "language 444 daniel 14u IPv4 0x123 0t0 TCP 127.0.0.1:43124 (LISTEN)\n"},
			wantError:    "all AI usage fallbacks failed: Antigravity local server returned HTTP 500.",
			wantRequests: []string{"43124/GetUnleashData", "43124/RetrieveUserQuotaSummary", "43124/GetUserStatus", "43124/GetCommandModelConfigs"},
			replies: map[string]antigravityOracleReply{
				"43124/GetUnleashData":           {status: http.StatusOK},
				"43124/RetrieveUserQuotaSummary": {status: http.StatusUnauthorized},
				"43124/GetUserStatus":            {status: http.StatusTooManyRequests, inputs: map[string]string{"Retry-After": "9"}},
				"43124/GetCommandModelConfigs":   {status: http.StatusInternalServerError},
			},
		},
	}
	fixture := AntigravityReportOracleFixture{SchemaVersion: ReportSchemaVersion}
	for _, input := range cases {
		transport := &antigravityOracleTransport{replies: input.replies}
		client := &http.Client{Transport: transport}
		provider := newAntigravityProvider(antigravityOracleProbe{processes: input.processes, portsByPID: input.ports}, client)
		report := BuildReport(context.Background(), []Provider{provider})
		if len(report.Providers) != 1 {
			return AntigravityReportOracleFixture{}, fmt.Errorf("Antigravity oracle %s returned %d rows", input.id, len(report.Providers))
		}
		row := report.Providers[0]
		if (row.Snapshot != nil) != input.wantSnapshot {
			return AntigravityReportOracleFixture{}, fmt.Errorf("Antigravity oracle %s snapshot presence = %t, want %t (error %q)", input.id, row.Snapshot != nil, input.wantSnapshot, row.Error)
		}
		if input.wantError != "" && row.Error != input.wantError {
			return AntigravityReportOracleFixture{}, fmt.Errorf("Antigravity oracle %s error = %q, want %q", input.id, row.Error, input.wantError)
		}
		gotRequests := make([]string, 0, len(transport.requests))
		for _, request := range transport.requests {
			parts := strings.Split(strings.Trim(request.URL.Path, "/"), "/")
			gotRequests = append(gotRequests, fmt.Sprintf("%s/%s", request.URL.Port(), parts[len(parts)-1]))
		}
		if strings.Join(gotRequests, ",") != strings.Join(input.wantRequests, ",") {
			return AntigravityReportOracleFixture{}, fmt.Errorf("Antigravity oracle %s requests = %v, want %v", input.id, gotRequests, input.wantRequests)
		}
		if row := &report.Providers[0]; row.Snapshot != nil {
			row.Snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
		}
		requests := make([]OracleRequest, 0, len(transport.requests))
		for _, request := range transport.requests {
			requests = append(requests, safeAntigravityOracleRequest(request))
		}
		fixture.Cases = append(fixture.Cases, AntigravityReportOracleCase{
			ID: input.id, ProcessList: input.processes, PortsByPID: input.ports, Responses: transport.responses, Report: report, Requests: requests,
		})
	}
	return fixture, nil
}

type oracleTransport struct {
	bodies    map[string][]byte
	sequences map[string][][]byte
	statuses  map[string][]int
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
	if sequence := t.statuses[oracleID(req)]; len(sequence) > 0 {
		seen := 0
		for _, request := range t.requests {
			if oracleID(request) == oracleID(req) {
				seen++
			}
		}
		index := seen - 1
		if index >= len(sequence) {
			index = len(sequence) - 1
		}
		status = sequence[index]
	}
	if status == 0 {
		status = http.StatusOK
	}
	return &http.Response{StatusCode: status, Body: ioNopCloser{Reader: bytes.NewReader(body)}, Header: make(http.Header), Request: req, ContentLength: int64(len(body))}
}

type kimiFallbackReportFixture struct {
	APIFailsCLIRecovers Report `json:"api_fails_cli_recovers"`
	APICLIFailWebWorks  Report `json:"api_cli_fail_web_works"`
}

// BuildKimiFallbackAuthenticatedReportOracle runs the shipped Kimi provider's
// API -> CLI and API -> CLI -> web chains with only temporary files and canned
// HTTP responses. It exercises the production constructor and report path.
func BuildKimiFallbackAuthenticatedReportOracle(apiBody, webBody []byte) (kimiFallbackReportFixture, error) {
	build := func(includeWeb bool) (Report, error) {
		home, err := os.MkdirTemp("", "symbrain-kimi-fallback-oracle-")
		if err != nil {
			return Report{}, fmt.Errorf("create isolated Kimi fallback home: %w", err)
		}
		defer os.RemoveAll(home)
		keys := []string{"HOME", "USERPROFILE", "KIMI_CODE_API_KEY", "KIMI_CODE_BASE_URL", "KIMI_CODE_HOME", "KIMI_AUTH_TOKEN"}
		type previous struct {
			value string
			set   bool
		}
		old := make(map[string]previous, len(keys))
		for _, key := range keys {
			value, set := os.LookupEnv(key)
			old[key] = previous{value: value, set: set}
			if err := os.Unsetenv(key); err != nil {
				return Report{}, fmt.Errorf("clear %s for Kimi fallback oracle: %w", key, err)
			}
		}
		defer func() {
			for key, item := range old {
				if item.set {
					_ = os.Setenv(key, item.value)
				} else {
					_ = os.Unsetenv(key)
				}
			}
		}()
		if err := os.Setenv("HOME", home); err != nil {
			return Report{}, fmt.Errorf("set isolated Kimi HOME: %w", err)
		}
		if err := os.Setenv("KIMI_CODE_API_KEY", "fallback-kimi-api-token"); err != nil {
			return Report{}, fmt.Errorf("set Kimi API oracle token: %w", err)
		}
		if includeWeb {
			if err := os.Setenv("KIMI_AUTH_TOKEN", "fallback-kimi-web-token"); err != nil {
				return Report{}, fmt.Errorf("set Kimi web oracle token: %w", err)
			}
		}
		credentialPath := filepath.Join(home, ".kimi-code", "credentials", "kimi-code.json")
		if err := os.MkdirAll(filepath.Dir(credentialPath), 0o700); err != nil {
			return Report{}, fmt.Errorf("create synthetic Kimi CLI directory: %w", err)
		}
		if err := os.WriteFile(credentialPath, []byte(`{"access_token":"fallback-kimi-cli-token"}`), 0o600); err != nil {
			return Report{}, fmt.Errorf("write synthetic Kimi CLI token: %w", err)
		}
		if err := os.WriteFile(filepath.Join(home, ".kimi-code", "device_id"), []byte("fallback-device-id\n"), 0o600); err != nil {
			return Report{}, fmt.Errorf("write synthetic Kimi device id: %w", err)
		}
		statuses := []int{http.StatusUnauthorized, http.StatusOK}
		bodies := [][]byte{apiBody, apiBody}
		wantRequests := 2
		if includeWeb {
			statuses = []int{http.StatusUnauthorized, http.StatusUnauthorized, http.StatusOK}
			bodies = append(bodies, webBody)
			wantRequests = 3
		}
		transport := &oracleTransport{
			sequences: map[string][][]byte{"kimi": bodies},
			statuses:  map[string][]int{"kimi": statuses},
		}
		client := &http.Client{Transport: roundTripFixture{transport}}
		provider := NewKimiProvider(client)
		got := strategySources(provider.Strategies())
		if includeWeb && (len(got) != 3 || got[0] != "api" || got[1] != "cli" || got[2] != "web") ||
			!includeWeb && (len(got) != 2 || got[0] != "api" || got[1] != "cli") {
			return Report{}, fmt.Errorf("Kimi fallback oracle has strategies %v", got)
		}
		report := BuildReport(context.Background(), []Provider{provider})
		if len(report.Providers) != 1 || report.Providers[0].Snapshot == nil || len(transport.requests) != wantRequests {
			return Report{}, fmt.Errorf("Kimi fallback oracle produced report=%d requests=%d, want one and %d", len(report.Providers), len(transport.requests), wantRequests)
		}
		if !includeWeb && (report.Providers[0].Snapshot.Source != "cli" || transport.requests[0].Header.Get("Authorization") != "Bearer fallback-kimi-api-token" || transport.requests[1].Header.Get("Authorization") != "Bearer fallback-kimi-cli-token" || transport.requests[1].Header.Get("X-Msh-Device-Id") != "fallback-device-id") {
			return Report{}, fmt.Errorf("Kimi CLI fallback did not use its stored credential")
		}
		if includeWeb && (report.Providers[0].Snapshot.Source != "web" || transport.requests[0].Header.Get("Authorization") != "Bearer fallback-kimi-api-token" || transport.requests[1].Header.Get("Authorization") != "Bearer fallback-kimi-cli-token" || transport.requests[1].Header.Get("X-Msh-Device-Id") != "fallback-device-id" || transport.requests[2].Header.Get("Authorization") != "Bearer fallback-kimi-web-token" || transport.requests[2].URL.Host != "www.kimi.com") {
			return Report{}, fmt.Errorf("Kimi web fallback did not use its environment credential")
		}
		report.Providers[0].Snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
		return report, nil
	}
	cli, err := build(false)
	if err != nil {
		return kimiFallbackReportFixture{}, err
	}
	web, err := build(true)
	if err != nil {
		return kimiFallbackReportFixture{}, err
	}
	return kimiFallbackReportFixture{APIFailsCLIRecovers: cli, APICLIFailWebWorks: web}, nil
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

// BuildNousFileAuthenticatedReportOracle constructs the shipped Nous provider
// from an isolated synthetic Hermes auth.json and drives it with canned HTTP
// responses. It never reads a user credential or contacts the portal.
func BuildNousFileAuthenticatedReportOracle(token string, body []byte, status int) (Report, error) {
	home, err := os.MkdirTemp("", "symbrain-nous-file-report-oracle-")
	if err != nil {
		return Report{}, fmt.Errorf("create isolated Nous file home: %w", err)
	}
	defer os.RemoveAll(home)
	keys := []string{"HOME", "USERPROFILE", "NOUS_PORTAL_ACCESS_TOKEN", "HERMES_HOME", "HERMES_PORTAL_BASE_URL"}
	type previous struct {
		value string
		set   bool
	}
	old := make(map[string]previous, len(keys))
	for _, key := range keys {
		value, set := os.LookupEnv(key)
		old[key] = previous{value: value, set: set}
		if err := os.Unsetenv(key); err != nil {
			return Report{}, fmt.Errorf("clear %s for Nous file oracle: %w", key, err)
		}
	}
	defer func() {
		for key, item := range old {
			if item.set {
				_ = os.Setenv(key, item.value)
			} else {
				_ = os.Unsetenv(key)
			}
		}
	}()
	for _, key := range []string{"HOME", "USERPROFILE"} {
		if err := os.Setenv(key, home); err != nil {
			return Report{}, fmt.Errorf("set isolated Nous file %s: %w", key, err)
		}
	}
	path := filepath.Join(home, ".hermes", "auth.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return Report{}, fmt.Errorf("create isolated Hermes auth directory: %w", err)
	}
	type providerEntry struct {
		ID        string `json:"id"`
		InvokeJWT string `json:"invoke_jwt"`
	}
	contents, err := json.Marshal(struct {
		Providers []providerEntry `json:"providers"`
	}{Providers: []providerEntry{{ID: "nous", InvokeJWT: token}}})
	if err != nil {
		return Report{}, fmt.Errorf("encode synthetic Hermes auth file: %w", err)
	}
	if err := os.WriteFile(path, contents, 0o600); err != nil {
		return Report{}, fmt.Errorf("write synthetic Hermes auth file: %w", err)
	}
	transport := &oracleTransport{bodies: map[string][]byte{"nous": body}, status: status}
	client := &http.Client{Transport: roundTripFixture{transport}}
	provider := NewNousPortalProvider(client)
	if !provider.IsConfigured() || provider.AuthStatus().Source != "file" || len(provider.Strategies()) != 1 || provider.Strategies()[0].Source() != "api" {
		return Report{}, fmt.Errorf("Nous file report oracle did not select its synthetic file credential")
	}
	report := BuildReport(context.Background(), []Provider{provider})
	if len(report.Providers) != 1 || len(transport.requests) != 1 || transport.requests[0].Header.Get("Authorization") != "Bearer "+token {
		return Report{}, fmt.Errorf("Nous file report oracle did not issue one authenticated request")
	}
	if report.Providers[0].Snapshot != nil {
		report.Providers[0].Snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	}
	return report, nil
}

// BuildNousEnvironmentPrecedenceReportOracle proves that the shipped
// environment credential wins over an existing default auth.json file.
func BuildNousEnvironmentPrecedenceReportOracle(body []byte) (Report, error) {
	home, err := os.MkdirTemp("", "symbrain-nous-precedence-oracle-")
	if err != nil {
		return Report{}, fmt.Errorf("create isolated Nous precedence home: %w", err)
	}
	defer os.RemoveAll(home)
	keys := []string{"HOME", "USERPROFILE", "NOUS_PORTAL_ACCESS_TOKEN", "HERMES_HOME", "HERMES_PORTAL_BASE_URL"}
	type previous struct {
		value string
		set   bool
	}
	old := make(map[string]previous, len(keys))
	for _, key := range keys {
		value, set := os.LookupEnv(key)
		old[key] = previous{value: value, set: set}
		if err := os.Unsetenv(key); err != nil {
			return Report{}, fmt.Errorf("clear %s for Nous precedence oracle: %w", key, err)
		}
	}
	defer func() {
		for key, item := range old {
			if item.set {
				_ = os.Setenv(key, item.value)
			} else {
				_ = os.Unsetenv(key)
			}
		}
	}()
	for key, value := range map[string]string{"HOME": home, "NOUS_PORTAL_ACCESS_TOKEN": "nous-env-precedence-token"} {
		if err := os.Setenv(key, value); err != nil {
			return Report{}, fmt.Errorf("set %s for Nous precedence oracle: %w", key, err)
		}
	}
	path := filepath.Join(home, ".hermes", "auth.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return Report{}, fmt.Errorf("create synthetic Hermes auth directory: %w", err)
	}
	if err := os.WriteFile(path, []byte(`{"providers":[{"id":"nous","access_token":"nous-file-shadow-token"}]}`), 0o600); err != nil {
		return Report{}, fmt.Errorf("write synthetic Hermes auth file: %w", err)
	}
	transport := &oracleTransport{bodies: map[string][]byte{"nous": body}}
	client := &http.Client{Transport: roundTripFixture{transport}}
	provider := NewNousPortalProvider(client)
	if !provider.IsConfigured() || provider.AuthStatus().Source != "env" || len(provider.Strategies()) != 1 || provider.Strategies()[0].Source() != "api" {
		return Report{}, fmt.Errorf("Nous precedence oracle did not choose the environment credential")
	}
	report := BuildReport(context.Background(), []Provider{provider})
	if len(report.Providers) != 1 || report.Providers[0].Snapshot == nil || len(transport.requests) != 1 || transport.requests[0].Header.Get("Authorization") != "Bearer nous-env-precedence-token" {
		return Report{}, fmt.Errorf("Nous precedence oracle did not issue one authenticated request with env token")
	}
	report.Providers[0].Snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	return report, nil
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

// BuildCombinedNativeAuthenticatedReportOracle assembles several supported
// production providers together from synthetic env and default CLI files.
// All HTTP requests use the fixture transport; Claude's keychain and process
// probes are injected as empty.
func BuildCombinedNativeAuthenticatedReportOracle(fixtures map[string][]byte) (Report, error) {
	home, err := os.MkdirTemp("", "symbrain-combined-usage-oracle-")
	if err != nil {
		return Report{}, fmt.Errorf("create isolated combined oracle home: %w", err)
	}
	defer os.RemoveAll(home)
	keys := []string{
		"HOME", "USERPROFILE", "ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN", "COPILOT_ACCESS_TOKEN",
		"OPENROUTER_API_KEY", "OPENROUTER_API_URL", "MOONSHOT_API_KEY", "MOONSHOT_REGION", "CURSOR_COOKIE",
		"KIMI_CODE_API_KEY", "KIMI_CODE_BASE_URL", "KIMI_CODE_HOME", "KIMI_AUTH_TOKEN",
		"NOUS_PORTAL_ACCESS_TOKEN", "HERMES_HOME", "HERMES_PORTAL_BASE_URL", "CODEX_ACCESS_TOKEN", "CODEX_HOME",
		"OPENCODE_COOKIE", "OPENCODE_WORKSPACE_ID",
	}
	type previous struct {
		value string
		set   bool
	}
	old := make(map[string]previous, len(keys))
	for _, key := range keys {
		value, set := os.LookupEnv(key)
		old[key] = previous{value: value, set: set}
		if err := os.Unsetenv(key); err != nil {
			return Report{}, fmt.Errorf("clear %s for combined oracle: %w", key, err)
		}
	}
	defer func() {
		for key, item := range old {
			if item.set {
				_ = os.Setenv(key, item.value)
			} else {
				_ = os.Unsetenv(key)
			}
		}
	}()
	for key, value := range map[string]string{
		"HOME": home, "KIMI_CODE_API_KEY": "combined-kimi-api-token", "KIMI_CODE_BASE_URL": "https://api.kimi.com/custom/v1",
		"MOONSHOT_API_KEY": "combined-moonshot-token", "MOONSHOT_REGION": "cn", "OPENROUTER_API_KEY": "combined-openrouter-token",
		"OPENROUTER_API_URL": "https://openrouter.ai/custom/v1", "OPENCODE_COOKIE": "combined-opencode-cookie",
		"OPENCODE_WORKSPACE_ID": "wrk_combined123",
	} {
		if err := os.Setenv(key, value); err != nil {
			return Report{}, fmt.Errorf("set %s for combined oracle: %w", key, err)
		}
	}
	kimiPath := filepath.Join(home, ".kimi-code", "credentials", "kimi-code.json")
	if err := os.MkdirAll(filepath.Dir(kimiPath), 0o700); err != nil {
		return Report{}, fmt.Errorf("create synthetic Kimi home: %w", err)
	}
	if err := os.WriteFile(kimiPath, []byte(`{"access_token":"combined-kimi-cli-token","refresh_token":"unused"}`), 0o600); err != nil {
		return Report{}, fmt.Errorf("write synthetic Kimi credentials: %w", err)
	}
	if err := os.WriteFile(filepath.Join(home, ".kimi-code", "device_id"), []byte("combined-device-id\n"), 0o600); err != nil {
		return Report{}, fmt.Errorf("write synthetic Kimi device id: %w", err)
	}
	nousPath := filepath.Join(home, ".hermes", "auth.json")
	if err := os.MkdirAll(filepath.Dir(nousPath), 0o700); err != nil {
		return Report{}, fmt.Errorf("create synthetic Nous home: %w", err)
	}
	if err := os.WriteFile(nousPath, []byte(`{"providers":[{"id":"nous","access_token":"combined-nous-file-token"}]}`), 0o600); err != nil {
		return Report{}, fmt.Errorf("write synthetic Nous credentials: %w", err)
	}

	combinedFixtures := make(map[string][]byte, len(fixtures))
	for name, body := range fixtures {
		combinedFixtures[name] = body
	}
	combinedFixtures["opencode"] = fixtures["opencode-stable"]
	combinedFixtures["kimi"] = fixtures["kimi-stable"]
	combinedFixtures["moonshot"] = fixtures["moonshot-cn"]
	transport := &oracleTransport{bodies: combinedFixtures}
	client := &http.Client{Transport: roundTripFixture{transport}}
	providers := allProviders(client, func() (string, *time.Time) { return "", nil }, oracleProbe{})
	if len(providers) != 10 {
		return Report{}, fmt.Errorf("combined oracle registered %d providers, want 10", len(providers))
	}
	for index, provider := range providers {
		wantConfigured := (index >= 4 && index <= 8) || index == 9
		if provider.IsConfigured() != wantConfigured {
			return Report{}, fmt.Errorf("combined oracle provider %s configured=%t, want %t", provider.ID(), provider.IsConfigured(), wantConfigured)
		}
	}
	if got := strategySources(providers[4].Strategies()); len(got) != 2 || got[0] != "api" || got[1] != "cli" || providers[4].AuthStatus().Source != "cli" {
		return Report{}, fmt.Errorf("combined oracle Kimi precedence is strategies=%v source=%q", got, providers[4].AuthStatus().Source)
	}
	if got := providers[6].AuthStatus().Source; got != "file" {
		return Report{}, fmt.Errorf("combined oracle Nous source is %q, want file", got)
	}
	if providers[7].AuthStatus().Source != "env" {
		return Report{}, fmt.Errorf("combined oracle OpenCode source is %q, want env", providers[7].AuthStatus().Source)
	}
	report := BuildReport(context.Background(), providers)
	if len(report.Providers) != 10 {
		return Report{}, fmt.Errorf("combined oracle produced %d report rows, want 10", len(report.Providers))
	}
	for _, index := range []int{4, 5, 6, 7, 8} {
		if report.Providers[index].Snapshot == nil {
			return Report{}, fmt.Errorf("combined oracle provider %s produced no snapshot", report.Providers[index].ID)
		}
		report.Providers[index].Snapshot.FetchedAt = time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
	}
	if len(transport.requests) != 5 {
		return Report{}, fmt.Errorf("combined oracle made %d fixture requests, want five", len(transport.requests))
	}
	findRequest := func(match func(*http.Request) bool) *http.Request {
		for _, request := range transport.requests {
			if match(request) {
				return request
			}
		}
		return nil
	}
	kimiRequest := findRequest(func(request *http.Request) bool {
		return request.URL.Host == "api.kimi.com" && request.URL.Path == "/custom/v1/coding/v1/usages" && request.Header.Get("Authorization") == "Bearer combined-kimi-api-token"
	})
	if kimiRequest == nil {
		return Report{}, fmt.Errorf("combined oracle Kimi did not try API strategy first")
	}
	if findRequest(func(request *http.Request) bool { return request.URL.Host == "api.moonshot.cn" }) == nil {
		return Report{}, fmt.Errorf("combined oracle Moonshot region was not cn")
	}
	if findRequest(func(request *http.Request) bool {
		return request.Header.Get("Authorization") == "Bearer combined-nous-file-token"
	}) == nil {
		return Report{}, fmt.Errorf("combined oracle Nous did not use the default credential file")
	}
	if findRequest(func(request *http.Request) bool {
		return request.URL.Query().Get("id") == openCodeSubscriptionServerID && request.URL.Query().Get("args") == `["wrk_combined123"]`
	}) == nil {
		return Report{}, fmt.Errorf("combined oracle OpenCode did not use the workspace override")
	}
	if findRequest(func(request *http.Request) bool {
		return request.URL.Host == "openrouter.ai" && request.URL.Path == "/custom/v1/auth/key"
	}) == nil {
		return Report{}, fmt.Errorf("combined oracle OpenRouter custom API base was not used")
	}
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

func safeAntigravityOracleRequest(req *http.Request) OracleRequest {
	request := safeOracleRequest(req)
	if token := req.Header.Get("X-Codeium-Csrf-Token"); token != "" {
		request.Headers["X-Codeium-Csrf-Token"] = token
	}
	return request
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
