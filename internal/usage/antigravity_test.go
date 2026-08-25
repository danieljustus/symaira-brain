package usage

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"reflect"
	"strings"
	"testing"
)

// MARK: - Test doubles

type stubProcessProbe struct {
	processes  string
	portsByPID map[int]string
}

func (s *stubProcessProbe) processList() (string, bool) { return s.processes, true }
func (s *stubProcessProbe) listeningPorts(pid int) (string, bool) {
	ports, ok := s.portsByPID[pid]
	return ports, ok
}
func (s *stubProcessProbe) isAntigravityRunning() bool {
	return strings.Contains(s.processes, "agy") || strings.Contains(s.processes, "Antigravity")
}

// scriptedPathTransport is keyed by the request path's last segment (the
// RPC method name), mirroring symaira-cockpit's ScriptedTransport test
// seam for the Antigravity local probe.
type scriptedPathTransport struct {
	script   map[string]scriptedResponse
	requests []*http.Request
}

func (t *scriptedPathTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, req)
	segments := strings.Split(req.URL.Path, "/")
	key := segments[len(segments)-1]
	r, ok := t.script[key]
	if !ok {
		return nil, errBoom
	}
	return &http.Response{
		StatusCode: r.status,
		Body:       io.NopCloser(bytes.NewReader(r.body)),
		Header:     make(http.Header),
	}, nil
}

// MARK: Candidate discovery (port discovery covered here)

func TestAntigravityParseCandidatesFindsAppServerWithCSRFToken(t *testing.T) {
	ps := "  123 /usr/libexec/sshd\n" +
		" 4567 /Applications/Antigravity.app/Contents/Resources/language_server_macos_arm --app_data_dir antigravity --csrf_token tok_abc123 --extension_server_port 34567\n" +
		" 9999 /usr/bin/ssh-agent\n"
	candidates := antigravityParseCandidates(ps)
	want := []antigravityServerCandidate{{pid: 4567, csrfToken: "tok_abc123"}}
	if !reflect.DeepEqual(candidates, want) {
		t.Errorf("candidates = %+v, want %+v", candidates, want)
	}
}

func TestAntigravityParseCandidatesFindsCLIWithoutToken(t *testing.T) {
	ps := "  7890 /opt/homebrew/bin/agy\n  1234 /usr/libexec/sshd\n"
	candidates := antigravityParseCandidates(ps)
	want := []antigravityServerCandidate{{pid: 7890, csrfToken: ""}}
	if !reflect.DeepEqual(candidates, want) {
		t.Errorf("candidates = %+v, want %+v", candidates, want)
	}
}

func TestAntigravityParseCandidatesIgnoresUnrelatedProcesses(t *testing.T) {
	ps := "  123 /usr/libexec/sshd\n" +
		"  456 /System/Library/CoreServices/Finder.app/Contents/MacOS/Finder\n" +
		"  789 /usr/bin/python3 /usr/local/bin/something language_server\n"
	if candidates := antigravityParseCandidates(ps); len(candidates) != 0 {
		t.Errorf("candidates = %+v, want empty", candidates)
	}
}

func TestAntigravityParsePortsExtractsListeningTCPPorts(t *testing.T) {
	lsof := "COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME\n" +
		"language_ 4567 daniel   14u  IPv4 0x123 0t0  TCP 127.0.0.1:34567 (LISTEN)\n" +
		"language_ 4567 daniel   15u  IPv6 0x456 0t0  TCP [::1]:34568 (LISTEN)\n" +
		"language_ 4567 daniel   16u  IPv4 0x789 0t0  TCP *:34569 (LISTEN)\n"
	got := antigravityParsePorts(lsof)
	want := []int{34567, 34568, 34569}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("ports = %v, want %v", got, want)
	}
}

func TestAntigravityParsePortsIgnoresNonListeners(t *testing.T) {
	lsof := "COMMAND     PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\n" +
		"sshd      1234 daniel    3u  IPv4 0xabc 0t0  TCP 127.0.0.1:22 (ESTABLISHED)\n"
	if got := antigravityParsePorts(lsof); len(got) != 0 {
		t.Errorf("ports = %v, want empty", got)
	}
}

// MARK: Parsing fixtures

func TestAntigravityParsesQuotaSummaryFixture(t *testing.T) {
	snap, err := antigravityQuotaSummarySnapshot(loadFixture("antigravity-quota-summary.json"), "antigravity", "local")
	if err != nil {
		t.Fatalf("antigravityQuotaSummarySnapshot(): %v", err)
	}
	if snap.Source != "local" {
		t.Errorf("Source = %q, want local", snap.Source)
	}
	weekly := meterByLabel(snap.Meters, "Gemini Models — Weekly limit")
	if weekly == nil || weekly.Used == nil || *weekly.Used != "58" {
		t.Errorf("weekly = %v, want used=58 ((1-0.42)*100)", weekly)
	}
	if weekly == nil || weekly.Unit != "%" {
		t.Errorf("weekly unit = %v, want %%", weekly)
	}
	if weekly == nil || weekly.ResetsAt == nil {
		t.Error("expected weekly ResetsAt to be set")
	}
	fiveHour := meterByLabel(snap.Meters, "Gemini Models — 5-hour limit")
	if fiveHour == nil || fiveHour.Used == nil || *fiveHour.Used != "15" {
		t.Errorf("fiveHour = %v, want used=15", fiveHour)
	}
	claude := meterByLabel(snap.Meters, "Claude and GPT models — Weekly limit")
	if claude == nil || claude.Used == nil || *claude.Used != "90" {
		t.Errorf("claude = %v, want used=90", claude)
	}
}

func TestAntigravityParsesUserStatusFixture(t *testing.T) {
	snap, err := antigravityUserStatusSnapshot(loadFixture("antigravity-user-status.json"), "antigravity", "local")
	if err != nil {
		t.Fatalf("antigravityUserStatusSnapshot(): %v", err)
	}
	gemini := meterByLabel(snap.Meters, "Gemini 2.5 Pro")
	if gemini == nil || gemini.Used == nil || *gemini.Used != "67" {
		t.Errorf("gemini = %v, want used=67", gemini)
	}
	claude := meterByLabel(snap.Meters, "claude-sonnet-4")
	if claude == nil || claude.Used == nil || *claude.Used != "50" {
		t.Errorf("claude = %v, want used=50", claude)
	}
}

func TestAntigravityParsesModelConfigsFixture(t *testing.T) {
	snap, err := antigravityModelConfigsSnapshot(loadFixture("antigravity-model-configs.json"), "antigravity", "local")
	if err != nil {
		t.Fatalf("antigravityModelConfigsSnapshot(): %v", err)
	}
	gptOSS := meterByLabel(snap.Meters, "GPT-OSS")
	if gptOSS == nil || gptOSS.Used == nil || *gptOSS.Used != "25" {
		t.Errorf("gptOSS = %v, want used=25", gptOSS)
	}
	if gptOSS == nil || gptOSS.ResetsAt == nil {
		t.Error("expected gptOSS ResetsAt to be set")
	}
}

// MARK: Strategy flow

func TestAntigravityNotRunningWhenNoCandidates(t *testing.T) {
	probe := &stubProcessProbe{processes: "  123 /usr/libexec/sshd\n"}
	strategy := &antigravityLocalProbeStrategy{probe: probe}

	_, err := strategy.Fetch(context.Background())
	agErr, ok := err.(*antigravityError)
	if !ok || agErr.kind != "not_running" {
		t.Fatalf("expected *antigravityError{kind: not_running}, got %v (%T)", err, err)
	}
}

func TestAntigravityConnectProbeThenQuotaSummary(t *testing.T) {
	probe := &stubProcessProbe{
		processes: "4567 /Applications/Antigravity.app/Contents/Resources/language_server_macos_arm --app_data_dir antigravity --csrf_token tok_abc123\n",
		portsByPID: map[int]string{
			4567: "language_ 4567 daniel   14u  IPv4 0x123 0t0  TCP 127.0.0.1:34567 (LISTEN)\n",
		},
	}
	client, transport := scriptedPathClientWithFixtures(map[string][]byte{
		"GetUnleashData":           nil,
		"RetrieveUserQuotaSummary": loadFixture("antigravity-quota-summary.json"),
	})
	strategy := &antigravityLocalProbeStrategy{probe: probe, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}

	if snap.ProviderID != "antigravity" {
		t.Errorf("ProviderID = %q, want antigravity", snap.ProviderID)
	}
	if snap.Source != "local" {
		t.Errorf("Source = %q, want local", snap.Source)
	}
	if meterByLabel(snap.Meters, "Gemini Models — Weekly limit") == nil {
		t.Error("expected a Gemini Models — Weekly limit meter")
	}
	for _, req := range transport.requests {
		if got := req.Header.Get("X-Codeium-Csrf-Token"); got != "tok_abc123" {
			t.Errorf("X-Codeium-Csrf-Token = %q, want tok_abc123 on every request", got)
		}
	}
	var connect *http.Request
	for _, req := range transport.requests {
		if strings.HasSuffix(req.URL.Path, "GetUnleashData") {
			connect = req
		}
	}
	if connect == nil {
		t.Fatal("expected a GetUnleashData request")
	}
	if got := connect.Header.Get("Connect-Protocol-Version"); got != "1" {
		t.Errorf("Connect-Protocol-Version = %q, want 1", got)
	}
	if len(transport.requests) != 2 {
		t.Errorf("requests = %d, want 2", len(transport.requests))
	}
}

func TestAntigravityQuotaChainFallsBackToUserStatus(t *testing.T) {
	probe := &stubProcessProbe{
		processes:  "7890 /opt/homebrew/bin/agy\n",
		portsByPID: map[int]string{7890: "agy      7890 daniel   14u  IPv4 0x123 0t0  TCP 127.0.0.1:34987 (LISTEN)\n"},
	}
	client, transport := scriptedPathClientWithFixtures(map[string][]byte{
		"GetUnleashData":           nil,
		"RetrieveUserQuotaSummary": []byte("not json"),
		"GetUserStatus":            loadFixture("antigravity-user-status.json"),
	})
	strategy := &antigravityLocalProbeStrategy{probe: probe, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}
	if snap.Source != "local" {
		t.Errorf("Source = %q, want local", snap.Source)
	}
	if meterByLabel(snap.Meters, "Gemini 2.5 Pro") == nil {
		t.Error("expected a Gemini 2.5 Pro meter")
	}
	nonConnect := 0
	for _, req := range transport.requests {
		if !strings.HasSuffix(req.URL.Path, "GetUnleashData") {
			nonConnect++
		}
	}
	if nonConnect != 2 {
		t.Errorf("non-connect requests = %d, want 2", nonConnect)
	}
}

func TestAntigravityQuotaChainEndsAtModelConfigs(t *testing.T) {
	probe := &stubProcessProbe{
		processes:  "7890 /opt/homebrew/bin/agy\n",
		portsByPID: map[int]string{7890: "agy      7890 daniel   14u  IPv4 0x123 0t0  TCP 127.0.0.1:34987 (LISTEN)\n"},
	}
	client, transport := scriptedPathClientWithFixtures(map[string][]byte{
		"GetUnleashData":           nil,
		"RetrieveUserQuotaSummary": []byte("not json"),
		"GetUserStatus":            []byte("not json"),
		"GetCommandModelConfigs":   loadFixture("antigravity-model-configs.json"),
	})
	strategy := &antigravityLocalProbeStrategy{probe: probe, client: client}

	snap, err := strategy.Fetch(context.Background())
	if err != nil {
		t.Fatalf("Fetch(): %v", err)
	}
	if meterByLabel(snap.Meters, "GPT-OSS") == nil {
		t.Error("expected a GPT-OSS meter")
	}
	if len(transport.requests) != 4 {
		t.Errorf("requests = %d, want 4", len(transport.requests))
	}
}

func TestAntigravityQuotaEndpointHTTP429MapsToRateLimitedWithRetryAfterHeader(t *testing.T) {
	probe := &stubProcessProbe{
		processes:  "7890 /opt/homebrew/bin/agy\n",
		portsByPID: map[int]string{7890: "agy      7890 daniel   14u  IPv4 0x123 0t0  TCP 127.0.0.1:34987 (LISTEN)\n"},
	}
	transport := &scriptedPathTransport{script: map[string]scriptedResponse{
		"GetUnleashData":           {status: 200},
		"RetrieveUserQuotaSummary": {status: 429},
		"GetUserStatus":            {status: 429},
		"GetCommandModelConfigs":   {status: 429},
	}}
	client := &http.Client{Transport: antigravityRetryAfterTransport{transport, "9"}}
	strategy := &antigravityLocalProbeStrategy{probe: probe, client: client}

	_, err := strategy.Fetch(context.Background())
	var rateLimited *RateLimitedError
	if !asRateLimited(err, &rateLimited) {
		t.Fatalf("expected *RateLimitedError, got %v (%T)", err, err)
	}
	if rateLimited.ProviderID != "antigravity" {
		t.Errorf("ProviderID = %q, want antigravity", rateLimited.ProviderID)
	}
	if rateLimited.RetryAfterSeconds == nil || *rateLimited.RetryAfterSeconds != 9 {
		t.Errorf("RetryAfterSeconds = %v, want 9", rateLimited.RetryAfterSeconds)
	}
}

// antigravityRetryAfterTransport wraps another transport and stamps a
// Retry-After header on every response — a thin decorator so
// scriptedPathTransport doesn't need per-response headers wired through.
type antigravityRetryAfterTransport struct {
	inner *scriptedPathTransport
	value string
}

func (t antigravityRetryAfterTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	resp, err := t.inner.RoundTrip(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode == 429 {
		resp.Header.Set("Retry-After", t.value)
	}
	return resp, nil
}

func TestAntigravityProviderIsAlwaysConfigured(t *testing.T) {
	p := NewAntigravityProvider()
	if !p.IsConfigured() {
		t.Error("expected always configured")
	}
	strategies := p.Strategies()
	if len(strategies) != 1 || strategies[0].Source() != "local" {
		t.Errorf("Strategies() = %v, want exactly [local]", strategies)
	}
}

// scriptedPathClientWithFixtures is a convenience wrapper: 200 OK for
// every listed method, with the given body (nil body -> empty 200).
func scriptedPathClientWithFixtures(byMethod map[string][]byte) (*http.Client, *scriptedPathTransport) {
	script := make(map[string]scriptedResponse, len(byMethod))
	for method, body := range byMethod {
		script[method] = scriptedResponse{status: 200, body: body}
	}
	transport := &scriptedPathTransport{script: script}
	return &http.Client{Transport: transport}, transport
}
