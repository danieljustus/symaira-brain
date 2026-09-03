//go:build darwin

package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"os/exec"
	"sort"
	"strings"
	"time"
)

// claudeKeychainService is the service name Claude Code stores its OAuth
// credentials under in the macOS login keychain. Current versions append a
// per-installation suffix ("Claude Code-credentials-552ffa86") that is not
// derivable from anything on disk, so the bare name is tried first and every
// suffixed variant found in the keychain after it.
const claudeKeychainService = "Claude Code-credentials"

// claudeKeychainTimeout bounds each `security` invocation. Reading an item
// this binary is not on the ACL of raises a macOS approval panel, and that
// panel blocks the subprocess until it is answered — a usage report must not
// hang on an unattended machine.
var claudeKeychainTimeout = 20 * time.Second

// securityCommand builds the `security` invocation; a var so tests can
// substitute a fake without a keychain.
var securityCommand = func(ctx context.Context, args ...string) *exec.Cmd {
	return exec.CommandContext(ctx, "security", args...)
}

// readClaudeKeychainCredential returns the Claude Code OAuth access token
// from the macOS login keychain, with the expiry the entry declares.
//
// This is the source Claude Code itself writes on macOS; the
// ~/.claude/.credentials.json file this provider also reads is what other
// platforms get. Returns an empty token when nothing usable is stored,
// which is not an error: not being signed in is a normal state.
func readClaudeKeychainCredential() (string, *time.Time) {
	if token, expiresAt, ok := readClaudeKeychainService(claudeKeychainService); ok {
		return token, expiresAt
	}
	// Only an install that does not use the bare name costs a keychain
	// listing.
	for _, service := range suffixedClaudeKeychainServices() {
		if token, expiresAt, ok := readClaudeKeychainService(service); ok {
			return token, expiresAt
		}
	}
	return "", nil
}

func readClaudeKeychainService(service string) (string, *time.Time, bool) {
	blob, err := runSecurity("find-generic-password", "-w", "-s", service)
	if err != nil {
		return "", nil, false
	}
	return parseClaudeKeychainBlob(blob)
}

// suffixedClaudeKeychainServices lists the per-installation service names
// present in the keychain, in a stable order.
//
// `security dump-keychain` lists item *attributes* only — it prints no
// secrets and raises no approval panel — and only the Claude service names
// are kept out of it.
func suffixedClaudeKeychainServices() []string {
	out, err := runSecurity("dump-keychain")
	if err != nil {
		return nil
	}

	seen := map[string]bool{}
	var services []string
	for _, line := range strings.Split(string(out), "\n") {
		name, ok := keychainServiceName(line)
		if !ok || seen[name] || !strings.HasPrefix(name, claudeKeychainService+"-") {
			continue
		}
		seen[name] = true
		services = append(services, name)
	}
	sort.Strings(services)
	return services
}

// keychainServiceName pulls the service out of a dump-keychain attribute
// line: `    "svce"<blob>="Claude Code-credentials-552ffa86"`.
func keychainServiceName(line string) (string, bool) {
	const marker = `"svce"<blob>="`
	start := strings.Index(line, marker)
	if start < 0 {
		return "", false
	}
	rest := line[start+len(marker):]
	end := strings.LastIndex(rest, `"`)
	if end <= 0 {
		return "", false
	}
	return rest[:end], true
}

// parseClaudeKeychainBlob reads the access token out of the stored JSON.
//
// The blob can also carry an `mcpOAuth` section holding tokens for MCP
// server logins. Those are not Claude subscription tokens and the usage
// endpoint rejects them, so an entry with `mcpOAuth` but no `claudeAiOauth`
// counts as "not signed in" rather than yielding a token that 401s — the
// same pitfall symaira-cockpit's Swift provider had to handle.
func parseClaudeKeychainBlob(blob []byte) (string, *time.Time, bool) {
	var root struct {
		ClaudeAIOAuth *struct {
			AccessToken string `json:"accessToken"`
			ExpiresAt   *int64 `json:"expiresAt"`
		} `json:"claudeAiOauth"`
	}
	if err := json.Unmarshal(bytes.TrimSpace(blob), &root); err != nil {
		return "", nil, false
	}
	if root.ClaudeAIOAuth == nil || root.ClaudeAIOAuth.AccessToken == "" {
		return "", nil, false
	}
	var expiresAt *time.Time
	if milliseconds := root.ClaudeAIOAuth.ExpiresAt; milliseconds != nil && *milliseconds > 0 {
		expiry := time.UnixMilli(*milliseconds).UTC()
		expiresAt = &expiry
	}
	return root.ClaudeAIOAuth.AccessToken, expiresAt, true
}

// runSecurity runs one `security` subcommand under the keychain timeout.
// Only stdout is returned; stderr is dropped, since the only failure that
// matters here ("item not found", "user denied access") is answered the same
// way — try the next service, then report "not signed in".
func runSecurity(args ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(context.Background(), claudeKeychainTimeout)
	defer cancel()

	cmd := securityCommand(ctx, args...)
	cmd.Stderr = nil
	return cmd.Output()
}
