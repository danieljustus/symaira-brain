package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
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
	bodies   map[string][]byte
	status   int
	requests []*http.Request
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
	body := t.bodies[id]
	status := t.status
	if status == 0 {
		status = http.StatusOK
	}
	return &http.Response{StatusCode: status, Body: http.NoBody, Header: make(http.Header), Request: req, ContentLength: int64(len(body))}, nil
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

type roundTripFixture struct{ target *oracleTransport }

func (r roundTripFixture) RoundTrip(req *http.Request) (*http.Response, error) {
	r.target.requests = append(r.target.requests, req.Clone(req.Context()))
	body := r.target.bodies[oracleID(req)]
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
