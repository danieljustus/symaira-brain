package usage

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strings"
	"time"
)

const (
	openCodeProviderID  = "opencode"
	openCodeDisplayName = "OpenCode Go"
)

// OpenCodeProvider — OpenCode Go usage provider.
//
// Tracks OpenCode Go subscription quota (rolling 5h window, weekly) from
// the web dashboard (POST https://opencode.ai/_server), reached via
// opencode.ai cookies. This pass ports only the web strategy
// (OPENCODE_COOKIE / OPENCODE_WORKSPACE_ID env vars). The Swift original's
// other source — a local SQLite history at
// ~/.local/share/opencode/opencode.db — is not ported for the same reason
// as CursorProvider's Cursor.app strategy: it needs a SQLite library, a
// dependency decision for its own follow-up. Mirrors symaira-cockpit's
// OpenCodeUsageProvider (web path only).
type OpenCodeProvider struct {
	cookieHeader      string
	credErr           error
	credSource        string
	workspaceOverride string
	client            *http.Client
}

// NewOpenCodeProvider reads OPENCODE_COOKIE and OPENCODE_WORKSPACE_ID from
// the environment.
func NewOpenCodeProvider(client *http.Client) *OpenCodeProvider {
	if client == nil {
		client = newProviderHTTPClient()
	}
	cookieHeader, credSource, credErr := resolveEnv("OPENCODE_COOKIE")
	return &OpenCodeProvider{
		cookieHeader:      cookieHeader,
		credErr:           credErr,
		credSource:        credSource,
		workspaceOverride: os.Getenv("OPENCODE_WORKSPACE_ID"),
		client:            client,
	}
}

func (p *OpenCodeProvider) ID() string          { return openCodeProviderID }
func (p *OpenCodeProvider) DisplayName() string { return openCodeDisplayName }
func (p *OpenCodeProvider) IsConfigured() bool {
	return p.cookieHeader != "" || p.workspaceOverride != ""
}

func (p *OpenCodeProvider) Strategies() []Strategy {
	if p.cookieHeader == "" {
		return nil
	}
	return []Strategy{&openCodeWebStrategy{cookieHeader: p.cookieHeader, workspaceOverride: p.workspaceOverride, client: p.client}}
}

func (p *OpenCodeProvider) AuthStatus() AuthStatus {
	if p.credErr != nil {
		return authErrStatus(p.credErr)
	}
	if p.cookieHeader != "" {
		return AuthStatus{Status: "available", Detail: "Cookie configured (OPENCODE_COOKIE)", Source: p.credSource}
	}
	if p.workspaceOverride != "" {
		return AuthStatus{Status: "available", Detail: "Workspace override set", Source: "env"}
	}
	return AuthStatus{Status: "missing", Detail: "No OpenCode Go credentials found — set OPENCODE_COOKIE or run OpenCode Go"}
}

type openCodeError struct {
	kind   string // invalid_credentials | network | api_error | parse_failed
	detail string
}

func (e *openCodeError) Error() string {
	switch e.kind {
	case "invalid_credentials":
		return "OpenCode session cookie is invalid or expired. Re-import the opencode.ai cookie."
	case "network":
		return fmt.Sprintf("OpenCode request failed: %s", e.detail)
	case "api_error":
		return fmt.Sprintf("OpenCode API error: %s", e.detail)
	default:
		return fmt.Sprintf("OpenCode returned an unreadable response: %s", e.detail)
	}
}

// openCodeWebStrategy fetches OpenCode Go quota from the opencode.ai web
// dashboard. GET/POST https://opencode.ai/_server with server-function IDs
// and the opencode.ai session cookie. Responses are text/javascript with
// serialized objects — parsed as JSON when possible, else via regex.
type openCodeWebStrategy struct {
	cookieHeader      string
	workspaceOverride string
	client            *http.Client
}

const (
	openCodeBaseURL              = "https://opencode.ai"
	openCodeServerURL            = "https://opencode.ai/_server"
	openCodeWorkspacesServerID   = "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f"
	openCodeSubscriptionServerID = "7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4"
	openCodeUserAgent            = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36"
)

func (s *openCodeWebStrategy) Source() string { return "web" }

func (s *openCodeWebStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	workspaceID := openCodeNormalizeWorkspaceID(s.workspaceOverride)
	if workspaceID == "" {
		id, err := s.fetchWorkspaceID(ctx)
		if err != nil {
			return nil, err
		}
		workspaceID = id
	}
	text, err := s.fetchSubscriptionInfo(ctx, workspaceID)
	if err != nil {
		return nil, err
	}
	return openCodeParseSubscription(text, time.Now().UTC())
}

// openCodeNormalizeWorkspaceID accepts a raw wrk_... ID, a full
// https://opencode.ai/workspace/... URL, or any text containing a wrk_...
// ID. Returns "" when none of those match.
func openCodeNormalizeWorkspaceID(raw string) string {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return ""
	}
	if strings.HasPrefix(trimmed, "wrk_") && len(trimmed) > 4 {
		return trimmed
	}
	if u, err := url.Parse(trimmed); err == nil {
		parts := strings.Split(strings.Trim(u.Path, "/"), "/")
		for i, part := range parts {
			if part == "workspace" && i+1 < len(parts) {
				candidate := parts[i+1]
				if strings.HasPrefix(candidate, "wrk_") && len(candidate) > 4 {
					return candidate
				}
			}
		}
	}
	if match := openCodeWorkspaceIDPattern.FindString(trimmed); match != "" {
		return match
	}
	return ""
}

var openCodeWorkspaceIDPattern = regexp.MustCompile(`wrk_[A-Za-z0-9]+`)

func (s *openCodeWebStrategy) fetchWorkspaceID(ctx context.Context) (string, error) {
	text, err := s.serverText(ctx, openCodeWorkspacesServerID, nil, http.MethodGet, openCodeBaseURL)
	if err != nil {
		return "", err
	}
	if openCodeLooksSignedOut(text) {
		return "", &openCodeError{kind: "invalid_credentials"}
	}
	if id := openCodeWorkspaceIDPattern.FindString(text); id != "" {
		return id, nil
	}
	fallback, err := s.serverText(ctx, openCodeWorkspacesServerID, []any{}, http.MethodPost, openCodeBaseURL)
	if err != nil {
		return "", err
	}
	if openCodeLooksSignedOut(fallback) {
		return "", &openCodeError{kind: "invalid_credentials"}
	}
	if id := openCodeWorkspaceIDPattern.FindString(fallback); id != "" {
		return id, nil
	}
	return "", &openCodeError{kind: "parse_failed", detail: "missing workspace id"}
}

func (s *openCodeWebStrategy) fetchSubscriptionInfo(ctx context.Context, workspaceID string) (string, error) {
	referer := openCodeBaseURL + "/workspace/" + workspaceID + "/billing"
	text, err := s.serverText(ctx, openCodeSubscriptionServerID, []any{workspaceID}, http.MethodGet, referer)
	if err != nil {
		return "", err
	}
	if openCodeLooksSignedOut(text) {
		return "", &openCodeError{kind: "invalid_credentials"}
	}
	if _, err := openCodeParseSubscription(text, time.Now().UTC()); err == nil {
		return text, nil
	}
	fallback, err := s.serverText(ctx, openCodeSubscriptionServerID, []any{workspaceID}, http.MethodPost, referer)
	if err != nil {
		return "", err
	}
	if openCodeLooksSignedOut(fallback) {
		return "", &openCodeError{kind: "invalid_credentials"}
	}
	return fallback, nil
}

func (s *openCodeWebStrategy) serverText(ctx context.Context, serverID string, args []any, method, referer string) (string, error) {
	var req *http.Request
	var err error
	if method == http.MethodGet {
		values := url.Values{}
		values.Set("id", serverID)
		if len(args) > 0 {
			if encoded, mErr := json.Marshal(args); mErr == nil {
				values.Set("args", string(encoded))
			}
		}
		req, err = http.NewRequestWithContext(ctx, http.MethodGet, openCodeServerURL+"?"+values.Encode(), nil)
	} else {
		var body io.Reader
		if args != nil {
			encoded, mErr := json.Marshal(args)
			if mErr != nil {
				return "", &openCodeError{kind: "network", detail: mErr.Error()}
			}
			body = strings.NewReader(string(encoded))
		}
		req, err = http.NewRequestWithContext(ctx, http.MethodPost, openCodeServerURL, body)
		if err == nil && args != nil {
			req.Header.Set("Content-Type", "application/json")
		}
	}
	if err != nil {
		return "", &openCodeError{kind: "network", detail: err.Error()}
	}

	req.Header.Set("Cookie", s.cookieHeader)
	req.Header.Set("X-Server-Id", serverID)
	req.Header.Set("X-Server-Instance", "server-fn:"+openCodeRandomID())
	req.Header.Set("User-Agent", openCodeUserAgent)
	req.Header.Set("Origin", openCodeBaseURL)
	req.Header.Set("Referer", referer)
	req.Header.Set("Accept", "text/javascript, application/json;q=0.9, */*;q=0.8")

	resp, err := doUsageRequest(ctx, s.client, req, false)
	if err != nil {
		return "", &openCodeError{kind: "network", detail: err.Error()}
	}
	data, err := readUsageBodyAndClose(resp.Body)
	if err != nil {
		return "", &openCodeError{kind: "network", detail: err.Error()}
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body := string(data)
		if resp.StatusCode == 401 || resp.StatusCode == 403 || openCodeLooksSignedOut(body) {
			return "", &openCodeError{kind: "invalid_credentials"}
		}
		return "", &openCodeError{kind: "api_error", detail: fmt.Sprintf("HTTP %d", resp.StatusCode)}
	}
	return string(data), nil
}

// openCodeRandomID generates a correlation id for X-Server-Instance — the
// server only needs a unique-looking token per request, not a validated
// UUID, so a plain random hex string avoids adding a UUID dependency.
func openCodeRandomID() string {
	buf := make([]byte, 16)
	if _, err := rand.Read(buf); err != nil {
		return "00000000-0000-0000-0000-000000000000"
	}
	return fmt.Sprintf("%x-%x-%x-%x-%x", buf[0:4], buf[4:6], buf[6:8], buf[8:10], buf[10:16])
}

// openCodeLooksSignedOut is a crude signed-out detection: sign-in prompts
// or auth errors in the serialized payload mean the cookie is stale.
func openCodeLooksSignedOut(text string) bool {
	lower := strings.ToLower(text)
	return strings.Contains(lower, "sign in") || strings.Contains(lower, "unauthorized") || strings.Contains(lower, "not authenticated")
}

// openCodeParseSubscription tries JSON first (serialized objects may be
// valid JSON), then a regex fallback for JS-literal payloads with
// unquoted keys.
