// Command opencode freezes the shipped OpenCode provider's executed request
// sequences against bounded, scripted responses. It complements
// scripts/usage-request-oracle (which records the default happy-path dump for
// every provider) with an OpenCode-specific discovery corpus: the GET-to-POST
// workspace and subscription fallbacks, signed-out detection, workspace-id
// normalization, and the provider-level error texts, each driven through the
// real usage.OpenCodeProvider with an injected recording RoundTripper.
//
// It never contacts the network, never reads a credential store, and prints
// machine-dependent values as placeholders so the output is stable across
// hosts. The recorded responses are deterministic canned bodies — this is a
// recording transport, not live provider evidence.
//
// Regenerate with:
//
//	go run ./scripts/usage-request-oracle/opencode
//
// Verify the committed fixture matches the generator byte-for-byte:
//
//	go run ./scripts/usage-request-oracle/opencode -check
//
// The fixture lives at rust/symbrain-usage/tests/fixtures/opencode_discovery.json
// and is never hand-edited.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/usage"
)

const (
	defaultOutput = "rust/symbrain-usage/tests/fixtures/opencode_discovery.json"

	// Dummy cookie, replaced with CREDENTIAL by normalize.
	fixtureCookie = "dump-opencode-cookie"

	// serverInstancePlaceholder replaces the per-request random
	// X-Server-Instance value (openCodeRandomID in internal/usage/opencode.go),
	// which carries no reproducible fact.
	serverInstancePlaceholder = "server-fn:<random>"

	// classNetwork marks the one error class whose detail text comes from the
	// HTTP client (Go's *url.Error wording) and cannot be reproduced
	// byte-for-byte by the port; the port compares the stable prefix only.
	classNetwork = "network"
)

// recordingTransport serves the case's script in order and records every
// request it sees. Consuming more steps than declared is recorded as an
// error instead of inventing a response. It never touches the network.
type recordingTransport struct {
	steps    []scriptStep
	consumed int
	requests []*http.Request
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
	if t.consumed >= len(t.steps) {
		return nil, errors.New("recording transport script exhausted")
	}
	step := t.steps[t.consumed]
	t.consumed++
	if step.Error != "" {
		return nil, errors.New(step.Error)
	}
	status := step.Status
	if status == 0 {
		status = 200
	}
	return &http.Response{
		StatusCode: status,
		Status:     fmt.Sprintf("%d %s", status, http.StatusText(status)),
		Header:     http.Header{"Content-Type": []string{"application/json"}},
		Body:       io.NopCloser(strings.NewReader(step.Body)),
		Request:    req,
	}, nil
}

func main() { os.Exit(run()) }

func run() int {
	check := flag.Bool("check", false, "fail if the committed fixture differs")
	output := flag.String("output", defaultOutput, "fixture path")
	flag.Parse()

	home, err := os.MkdirTemp("", "opencode-discovery-oracle-")
	if err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle: temp home:", err)
		return 1
	}
	defer func() { _ = os.RemoveAll(home) }()

	// Isolate the provider from the ambient environment before the first
	// provider construction: NewOpenCodeProvider reads these two variables,
	// and an ambient secret reference would otherwise resolve through the
	// shared secret store.
	os.Setenv("HOME", home)
	os.Unsetenv("OPENCODE_COOKIE")
	os.Unsetenv("OPENCODE_WORKSPACE_ID")

	testdata := filepath.Join("internal", "usage", "testdata")
	bodyWorkspaces, err = readTestdata(testdata, "opencode-workspaces.txt")
	if err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle:", err)
		return 1
	}
	bodySubscription, err = readTestdata(testdata, "opencode-subscription-json.txt")
	if err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle:", err)
		return 1
	}

	declared := append(cases(), reviewCases()...)
	out := dump{
		SchemaVersion: 2,
		Provenance: provenance{
			Generator:    "scripts/usage-request-oracle/opencode/main.go",
			Command:      "go run ./scripts/usage-request-oracle/opencode",
			CheckCommand: "go run ./scripts/usage-request-oracle/opencode -check",
			OracleSource: "internal/usage/opencode.go",
			OracleSources: []string{
				"internal/usage/opencode.go",
				"internal/usage/opencode_parse.go",
			},
			OracleAnchors: []string{
				"openCodeWorkspacesServerID",
				"def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f",
				"7abeebee372f304e050aaaf92be863f4a86490e382f8c79db68fd94040d691b4",
				"OpenCode session cookie is invalid or expired. Re-import the opencode.ai cookie.",
				"OpenCode request failed: ",
				"OpenCode API error: ",
				"missing workspace id",
				"missing usage fields",
				"openCodeLooksSignedOut",
				"wrk_[A-Za-z0-9]+",
				"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36",
				"text/javascript, application/json;q=0.9, */*;q=0.8",
			},
			Testdata: []string{
				"internal/usage/testdata/opencode-workspaces.txt",
				"internal/usage/testdata/opencode-subscription-json.txt",
			},
			Transport: "deterministic recording transport: injected http.RoundTripper with canned responses; no network, no live provider data",
		},
		Cases: make([]recordedCase, 0, len(declared)),
	}

	if err := recordProvenance(&out.Provenance); err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle:", err)
		return 1
	}
	declaredIDs := make(map[string]bool, len(declared))
	for _, tc := range declared {
		if declaredIDs[tc.id] {
			fmt.Fprintln(os.Stderr, "opencode-discovery-oracle: duplicate case id:", tc.id)
			return 1
		}
		declaredIDs[tc.id] = true
		out.Cases = append(out.Cases, runCase(tc))
	}
	if len(out.Cases) != len(declared) {
		fmt.Fprintf(os.Stderr, "opencode-discovery-oracle: executed %d cases, declared %d\n", len(out.Cases), len(declared))
		return 1
	}

	encoded, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle: encode:", err)
		return 1
	}
	encoded = append(encoded, '\n')

	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, encoded) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/usage-request-oracle/opencode\n", *output)
			return 1
		}
		return 0
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle: mkdir:", err)
		return 1
	}
	if err := os.WriteFile(*output, encoded, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, "opencode-discovery-oracle: write:", err)
		return 1
	}
	fmt.Fprintln(os.Stderr, "wrote", *output)
	return 0
}

// runCase drives the real provider once and records what actually executed.
func runCase(tc testCase) recordedCase {
	if tc.cookie {
		_ = os.Setenv("OPENCODE_COOKIE", fixtureCookie)
	} else {
		_ = os.Unsetenv("OPENCODE_COOKIE")
	}
	if tc.workspace != nil {
		_ = os.Setenv("OPENCODE_WORKSPACE_ID", *tc.workspace)
	} else {
		_ = os.Unsetenv("OPENCODE_WORKSPACE_ID")
	}

	env := map[string]string{}
	if tc.cookie {
		env["OPENCODE_COOKIE"] = fixtureCookie
	}
	if tc.workspace != nil {
		env["OPENCODE_WORKSPACE_ID"] = *tc.workspace
	}

	transport := &recordingTransport{steps: tc.script}
	client := &http.Client{Transport: transport, Timeout: 5 * time.Second}
	provider := usage.NewOpenCodeProvider(client)

	snapshot, fetchErr := usage.RunStrategyChain(context.Background(), provider.Strategies())

	entry := recordedCase{
		ID:        tc.id,
		Env:       env,
		Script:    tc.script,
		Workspace: observedWorkspace(transport.requests),
		Executed:  capture(transport.requests),
	}
	if fetchErr != nil {
		entry.Result = result{Kind: "error", Text: normalize(fetchErr.Error())}
		// The class is derived from the shipped text itself: serverText wraps
		// every transport failure as an openCodeError kind=network.
		if strings.Contains(fetchErr.Error(), "OpenCode request failed: ") {
			entry.Result.Class = classNetwork
		}
	} else if snapshot != nil {
		entry.Result = result{Kind: "ok", Snapshot: summarize(snapshot)}
	} else {
		entry.Result = result{Kind: "error", Text: normalize("oracle returned no snapshot and no error")}
	}
	return entry
}

// observedWorkspace derives the normalized workspace id the strategy used
// from the first executed request: a request without an `args` query is the
// workspace lookup (empty id), a request with `args` carries the id. It is
// nil when no request ran at all (workspace-only configuration).
func observedWorkspace(requests []*http.Request) *string {
	if len(requests) == 0 {
		return nil
	}
	rawArgs := requests[0].URL.Query().Get("args")
	if rawArgs == "" {
		return ws("")
	}
	var args []string
	if err := json.Unmarshal([]byte(rawArgs), &args); err != nil || len(args) == 0 {
		return ws("")
	}
	return ws(args[0])
}

func summarize(snap *usage.UsageSnapshot) *snapshotSummary {
	out := &snapshotSummary{Source: snap.Source, Meters: make([]meterSummary, 0, len(snap.Meters))}
	for _, meter := range snap.Meters {
		var resetAfterNS *int64
		if meter.ResetsAt != nil {
			delta := meter.ResetsAt.Sub(snap.FetchedAt).Nanoseconds()
			resetAfterNS = &delta
		}
		out.Meters = append(out.Meters, meterSummary{
			Label:        meter.Label,
			Used:         meter.Used,
			Limit:        meter.Limit,
			Unit:         meter.Unit,
			ResetsAt:     meter.ResetsAt != nil,
			ResetAfterNS: resetAfterNS,
		})
	}
	return out
}

func readTestdata(dir, name string) (string, error) {
	data, err := os.ReadFile(filepath.Join(dir, name))
	if err != nil {
		return "", fmt.Errorf("read testdata %s: %w", name, err)
	}
	return string(data), nil
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
			value := normalize(strings.Join(req.Header.Values(name), ", "))
			if name == "X-Server-Instance" && strings.HasPrefix(value, "server-fn:") {
				value = serverInstancePlaceholder
			}
			entry.Headers = append(entry.Headers, headerPair{Name: name, Value: value})
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

// Only the fixture cookie and random request instance are normalized. Never
// replace host/version/platform substrings inside behavior-bearing payloads.
func normalize(text string) string {
	return strings.ReplaceAll(text, fixtureCookie, "CREDENTIAL")
}
