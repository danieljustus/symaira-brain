// Command usage-request-oracle dumps the HTTP request every shipped usage
// strategy would send — method, URL, headers, body — plus the provider-level
// error text each strategy chain produces for a canned HTTP status. The Rust
// port pins its request layer against this output (see the request oracle test
// in rust/symbrain-usage), because the differential suite cannot observe that
// layer: a real fetch needs a live endpoint and a working credential.
//
// Regenerate with:
//
//	go run ./scripts/usage-request-oracle > \
//	  rust/symbrain-usage/tests/fixtures/usage_requests.json
//
// The fixture credentials below are dummies; no real secret is read or printed.
// Values that are machine-dependent (hostname, runtime version) are printed as
// placeholders so the output is stable across machines. It complements
// scripts/usage-oracle, which freezes the response-parsing layer.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/usage"
)

// Dummy credentials, chosen so the dump shows which header carries the value
// and where inside the value it sits.
const (
	fixtureClaudeAdmin = "dump-claude-admin"
	fixtureClaudeOAuth = "dump-claude-oauth"
	fixtureCodex       = "dump-codex-oauth"
	fixtureCopilot     = "dump-copilot-oauth"
	fixtureCursor      = "dump-cursor-cookie"
	fixtureKimiAPIKey  = "dump-kimi-api-key"
	fixtureKimiWeb     = "dump-kimi-web-token"
	fixtureKimiDevice  = "dump-kimi-device-id"
	fixtureMoonshot    = "dump-moonshot-key"
	fixtureNous        = "dump-nous-token"
	fixtureOpenCode    = "dump-opencode-cookie"
	fixtureOpenRouter  = "dump-openrouter-key"
)

// cannedResponses are the statuses probed for their shipped error text.
var cannedResponses = []struct {
	label  string
	status int
	body   string
}{
	{"unauthorized", 401, `{"error":"nope"}`},
	{"rate-limited", 429, `{"error":"slow down"}`},
	{"server-error", 500, "oops"},
	{"malformed", 200, "not json at all"},
}

// headerPair keeps the dump explicit: a JSON object would lose ordering and
// duplicate names, and the port compares the pairs one by one.
type headerPair struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type capturedRequest struct {
	Method  string       `json:"method"`
	URL     string       `json:"url"`
	Headers []headerPair `json:"headers"`
	Body    string       `json:"body,omitempty"`
}

type strategyDump struct {
	Provider string            `json:"provider"`
	Source   string            `json:"source"`
	Requests []capturedRequest `json:"requests"`
	Error    string            `json:"error,omitempty"`
}

type chainDump struct {
	Provider string `json:"provider"`
	Status   string `json:"status"`
	Text     string `json:"text"`
}

type dump struct {
	Requests []strategyDump `json:"requests"`
	Chains   []chainDump    `json:"chains"`
}

// recordingTransport captures every request a strategy makes and answers with a
// canned response. It never touches the network.
type recordingTransport struct {
	requests []*http.Request
	status   int
	body     string
}

func (t *recordingTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	clone := req.Clone(req.Context())
	clone.Header = req.Header.Clone()
	if req.Body != nil {
		data, _ := io.ReadAll(req.Body)
		_ = req.Body.Close()
		clone.Body = io.NopCloser(strings.NewReader(string(data)))
	}
	t.requests = append(t.requests, clone)
	status := t.status
	if status == 0 {
		status = 200
	}
	body := t.body
	if body == "" {
		body = "{}"
	}
	return &http.Response{
		StatusCode: status,
		Status:     fmt.Sprintf("%d %s", status, http.StatusText(status)),
		Header:     http.Header{"Content-Type": []string{"application/json"}},
		Body:       io.NopCloser(strings.NewReader(body)),
		Request:    req,
	}, nil
}

func main() {
	home, err := os.MkdirTemp("", "usage-request-oracle-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "usage-request-oracle: temp home:", err)
		os.Exit(1)
	}
	defer func() { _ = os.RemoveAll(home) }()
	if err := prepareFixtureHome(home); err != nil {
		fmt.Fprintln(os.Stderr, "usage-request-oracle: fixture home:", err)
		os.Exit(1)
	}
	applyFixtureEnv(home)

	transport := &recordingTransport{}
	client := &http.Client{Transport: transport, Timeout: 5 * time.Second}
	providers := usage.AllProviders(client)

	out := dump{}
	for _, provider := range providers {
		if len(provider.Strategies()) == 0 {
			out.Requests = append(out.Requests, strategyDump{
				Provider: provider.ID(),
				Source:   "none",
			})
			continue
		}
		for _, strategy := range provider.Strategies() {
			transport.requests = nil
			_, fetchErr := strategy.Fetch(context.Background())
			entry := strategyDump{
				Provider: provider.ID(),
				Source:   strategy.Source(),
				Requests: capture(transport.requests),
			}
			if fetchErr != nil {
				entry.Error = normalize(fetchErr.Error())
			}
			out.Requests = append(out.Requests, entry)
		}
	}

	for _, provider := range providers {
		for _, canned := range cannedResponses {
			transport.requests = nil
			transport.status = canned.status
			transport.body = canned.body
			_, chainErr := usage.RunStrategyChain(context.Background(), provider.Strategies())
			transport.status = 0
			transport.body = ""
			text := ""
			if chainErr != nil {
				text = normalize(chainErr.Error())
			}
			out.Chains = append(out.Chains, chainDump{
				Provider: provider.ID(),
				Status:   canned.label,
				Text:     text,
			})
		}
	}

	encoded, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, "usage-request-oracle: encode:", err)
		os.Exit(1)
	}
	fmt.Println(string(encoded))
}

func capture(requests []*http.Request) []capturedRequest {
	out := make([]capturedRequest, 0, len(requests))
	for _, req := range requests {
		entry := capturedRequest{
			Method: req.Method,
			URL:    normalize(req.URL.String()),
			Body:   body(req),
		}
		for _, name := range sortedHeaderNames(req.Header) {
			entry.Headers = append(entry.Headers, headerPair{
				Name:  name,
				Value: normalize(strings.Join(req.Header.Values(name), ", ")),
			})
		}
		out = append(out, entry)
	}
	return out
}

func body(req *http.Request) string {
	if req.GetBody == nil {
		return ""
	}
	reader, err := req.GetBody()
	if err != nil {
		return "?"
	}
	defer func() { _ = reader.Close() }()
	data, err := io.ReadAll(reader)
	if err != nil {
		return "?"
	}
	return normalize(string(data))
}

func sortedHeaderNames(header http.Header) []string {
	names := make([]string, 0, len(header))
	for name := range header {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

// normalize replaces machine-dependent values so the dump is diffable on any
// host: the build's hostname, the runtime version a shipped binary would
// report, the temp fixture home, and every dummy credential.
func normalize(text string) string {
	if host, err := os.Hostname(); err == nil && host != "" {
		text = strings.ReplaceAll(text, host, "<host>")
	}
	version := strings.TrimPrefix(runtime.Version(), "go")
	text = strings.ReplaceAll(text, version, "<version>")
	if home := os.Getenv("HOME"); home != "" {
		text = strings.ReplaceAll(text, home, "<home>")
	}
	for _, credential := range []string{
		fixtureClaudeOAuth, fixtureClaudeAdmin, fixtureCodex, fixtureCopilot, fixtureCursor,
		fixtureKimiAPIKey, fixtureKimiWeb, fixtureMoonshot, fixtureNous, fixtureOpenCode,
		fixtureOpenRouter,
	} {
		text = strings.ReplaceAll(text, credential, "CREDENTIAL")
	}
	return text
}

func applyFixtureEnv(home string) {
	os.Setenv("HOME", home)
	os.Setenv("CODEX_HOME", filepath.Join(home, ".codex"))
	os.Setenv("KIMI_CODE_HOME", filepath.Join(home, ".kimi-code"))
	os.Setenv("HERMES_HOME", filepath.Join(home, ".hermes"))
	os.Setenv("ANTHROPIC_ADMIN_KEY", fixtureClaudeAdmin)
	os.Setenv("ANTHROPIC_OAUTH_TOKEN", fixtureClaudeOAuth)
	os.Setenv("CODEX_ACCESS_TOKEN", fixtureCodex)
	os.Setenv("COPILOT_ACCESS_TOKEN", fixtureCopilot)
	os.Setenv("CURSOR_COOKIE", fixtureCursor)
	os.Setenv("KIMI_AUTH_TOKEN", fixtureKimiWeb)
	os.Setenv("MOONSHOT_API_KEY", fixtureMoonshot)
	os.Setenv("NOUS_PORTAL_ACCESS_TOKEN", fixtureNous)
	os.Setenv("OPENCODE_COOKIE", fixtureOpenCode)
	os.Setenv("OPENROUTER_API_KEY", fixtureOpenRouter)
	os.Setenv("OPENCODE_WORKSPACE_ID", "dump-workspace")
	// Not set on purpose: KIMI_CODE_API_KEY is written to the CLI store instead,
	// so the dump covers the CLI strategy rather than the API-key strategy.
	_ = os.Unsetenv("KIMI_CODE_API_KEY")
	_ = os.Unsetenv("KIMI_CODE_BASE_URL")
	_ = os.Unsetenv("OPENROUTER_API_URL")
	_ = os.Unsetenv("HERMES_PORTAL_BASE_URL")
	_ = os.Unsetenv("MOONSHOT_REGION")
}

// prepareFixtureHome writes one credential file per file-based source into the
// fixture home. The formats mirror what the shipped readers expect.
func prepareFixtureHome(home string) error {
	files := map[string]string{
		filepath.Join(home, ".claude", ".credentials.json"):                `{"claudeAiOauth":{"accessToken":"` + fixtureClaudeOAuth + `"}}`,
		filepath.Join(home, ".codex", "auth.json"):                         `{"tokens":{"access_token":"` + fixtureCodex + `"}}`,
		filepath.Join(home, ".config", "gh", "hosts.yml"):                  "github.com:\n    oauth_token: " + fixtureCopilot + "\n",
		filepath.Join(home, ".kimi-code", "credentials", "kimi-code.json"): `{"access_token":"` + fixtureKimiAPIKey + `"}`,
		filepath.Join(home, ".kimi-code", "device_id"):                     fixtureKimiDevice + "\n",
		filepath.Join(home, ".hermes", "auth.json"):                        `{"access_token":"` + fixtureNous + `"}`,
	}
	for path, content := range files {
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			return err
		}
	}
	return nil
}
