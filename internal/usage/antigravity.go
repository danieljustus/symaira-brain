package usage

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
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

type contextProcessProbe interface {
	processListContext(context.Context) (string, bool)
	listeningPortsContext(context.Context, int) (string, bool)
}

const maxProcessProbeBytes = 64 << 10

// shellProcessProbe is the production probe, backed by bounded, cancellable
// ps and lsof commands on PATH. Output is redirected to a temporary file so a
// child or descendant cannot hold a pipe reader open after cancellation.
type shellProcessProbe struct{}

func (shellProcessProbe) processList() (string, bool) {
	return shellProcessProbe{}.processListContext(context.Background())
}

func (shellProcessProbe) listeningPorts(pid int) (string, bool) {
	return shellProcessProbe{}.listeningPortsContext(context.Background(), pid)
}

func (shellProcessProbe) processListContext(ctx context.Context) (string, bool) {
	path, ok := resolveProbeTool("ps")
	if !ok {
		return "", false
	}
	return boundedCommandOutput(ctx, func(ctx context.Context) *exec.Cmd {
		return exec.CommandContext(ctx, path, "-ax", "-o", "pid=,command=") // #nosec G204 -- path is an absolute LookPath result for the fixed basename ps; arguments are fixed and no shell is used.
	})
}

func (shellProcessProbe) listeningPortsContext(ctx context.Context, pid int) (string, bool) {
	if pid <= 0 || pid > 1<<31-1 {
		return "", false
	}
	path, ok := resolveProbeTool("lsof")
	if !ok {
		return "", false
	}
	return boundedCommandOutput(ctx, func(ctx context.Context) *exec.Cmd {
		return exec.CommandContext(ctx, path, "-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", strconv.Itoa(pid)) // #nosec G204 -- path is an absolute LookPath result for fixed basename lsof; pid is a bounded integer argv element and no shell is used.
	})
}

func resolveProbeTool(name string) (string, bool) {
	path, err := exec.LookPath(name)
	if err != nil || !filepath.IsAbs(path) || filepath.Base(path) != name {
		return "", false
	}
	return filepath.Clean(path), true
}

func (s shellProcessProbe) isAntigravityRunning() bool {
	list, ok := s.processList()
	if !ok {
		return false
	}
	return strings.Contains(list, "agy") || strings.Contains(list, "Antigravity")
}

func boundedCommandOutput(parent context.Context, command func(context.Context) *exec.Cmd) (string, bool) {
	const commandTimeout = 2 * time.Second
	ctx, cancel := context.WithTimeout(parent, commandTimeout)
	defer cancel()
	file, err := os.CreateTemp("", "symbrain-usage-probe-*")
	if err != nil {
		return "", false
	}
	path := file.Name()
	defer os.Remove(path)
	cmd := command(ctx)
	configureProbeProcess(cmd)
	cmd.Stdout = file
	cmd.Stderr = io.Discard
	if err := cmd.Start(); err != nil {
		if closeErr := file.Close(); closeErr != nil {
			return "", false
		}
		return "", false
	}
	wait := make(chan error, 1)
	go func() { wait <- cmd.Wait() }()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case err = <-wait:
			if err != nil {
				closeErr := file.Close()
				if closeErr != nil {
					return "", false
				}
				return "", false
			}
			if _, err = file.Seek(0, io.SeekStart); err != nil {
				if closeErr := file.Close(); closeErr != nil {
					return "", false
				}
				return "", false
			}
			data, readErr := io.ReadAll(io.LimitReader(file, maxProcessProbeBytes+1))
			closeErr := file.Close()
			if readErr != nil || closeErr != nil || len(data) > maxProcessProbeBytes {
				return "", false
			}
			return string(data), true
		case <-ctx.Done():
			killProbeProcess(cmd)
			waitErr := <-wait
			closeErr := file.Close()
			if waitErr != nil || closeErr != nil {
				return "", false
			}
			return "", false
		case <-ticker.C:
			info, statErr := file.Stat()
			if statErr != nil || info.Size() > maxProcessProbeBytes {
				killProbeProcess(cmd)
				waitErr := <-wait
				closeErr := file.Close()
				if waitErr != nil || closeErr != nil {
					return "", false
				}
				return "", false
			}
		}
	}
}

func antigravityProcessList(ctx context.Context, probe antigravityProcessProbe) (string, bool) {
	if contextProbe, ok := probe.(contextProcessProbe); ok {
		return contextProbe.processListContext(ctx)
	}
	return probe.processList()
}

func antigravityListeningPorts(ctx context.Context, probe antigravityProcessProbe, pid int) (string, bool) {
	if contextProbe, ok := probe.(contextProcessProbe); ok {
		return contextProbe.listeningPortsContext(ctx, pid)
	}
	return probe.listeningPorts(pid)
}

// MARK: - Local transport

// Antigravity uses the same verified TLS stack as remote providers. The local
// endpoint is explicitly allowed by doUsageRequest, but certificate
// verification is never disabled and redirects are never followed.
func newAntigravityHTTPClient() *http.Client {
	return newProviderHTTPClient()
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
	list, _ := antigravityProcessList(ctx, s.probe)
	candidates := antigravityParseCandidates(list)
	if len(candidates) == 0 {
		return nil, &antigravityError{kind: "not_running"}
	}

	var lastErr error = &antigravityError{kind: "not_running"}
	client := s.httpClient()
	for _, candidate := range candidates {
		portList, _ := antigravityListeningPorts(ctx, s.probe, candidate.pid)
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

	connectResp, err := doUsageRequest(ctx, client, connectReq, true)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: fmt.Sprintf("connect probe failed on port %d", port)}
	}
	if err := connectResp.Body.Close(); err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: fmt.Sprintf("connect probe failed on port %d", port)}
	}
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

	resp, err := doUsageRequest(ctx, client, req, true)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: err.Error()}
	}
	data, err := readUsageBodyAndClose(resp.Body)
	if err != nil {
		return nil, &antigravityError{kind: "probe_failed", detail: err.Error()}
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if resp.StatusCode == 429 {
			return nil, &RateLimitedError{ProviderID: antigravityProviderID, RetryAfterSeconds: parseRetryAfter(resp.Header.Get("Retry-After"))}
		}
		return nil, &antigravityError{kind: "http_status", status: resp.StatusCode}
	}
	return data, nil
}
