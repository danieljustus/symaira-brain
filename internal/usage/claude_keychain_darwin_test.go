//go:build darwin

//nolint:gosec // G204: the fixed-binary re-exec is the standard helper-process test pattern.
package usage

import (
	"context"
	"os"
	"os/exec"
	"reflect"
	"testing"
	"time"
)

// TestHelperSecurity stands in for the `security` binary: it prints what the
// fake invocation put in SECURITY_HELPER_STDOUT and exits with
// SECURITY_HELPER_STATUS.
func TestHelperSecurity(t *testing.T) {
	if os.Getenv("USAGE_HELPER_PROCESS") != "security" {
		t.Skip("helper subprocess target")
	}
	if _, err := os.Stdout.WriteString(os.Getenv("SECURITY_HELPER_STDOUT")); err != nil {
		os.Exit(2)
	}
	if os.Getenv("SECURITY_HELPER_STATUS") == "fail" {
		os.Exit(1)
	}
	os.Exit(0)
}

// fakeSecurity replaces the `security` invocation with a helper process that
// prints the output registered for the subcommand, and records every
// invocation's arguments.
func fakeSecurity(t *testing.T, outputs map[string]string) *[][]string {
	t.Helper()
	original := securityCommand
	var calls [][]string
	securityCommand = func(ctx context.Context, args ...string) *exec.Cmd {
		calls = append(calls, append([]string(nil), args...))
		out, ok := outputs[args[0]]
		cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=TestHelperSecurity")
		cmd.Env = append(os.Environ(),
			"USAGE_HELPER_PROCESS=security",
			"SECURITY_HELPER_STDOUT="+out,
		)
		if !ok {
			cmd.Env = append(cmd.Env, "SECURITY_HELPER_STATUS=fail")
		}
		return cmd
	}
	t.Cleanup(func() { securityCommand = original })
	return &calls
}

const claudeKeychainDump = `keychain: "/Users/dev/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "acct"<blob>="dev"
    "svce"<blob>="Some Other App"
class: "genp"
attributes:
    "acct"<blob>="dev"
    "svce"<blob>="Claude Code-credentials-552ffa86"
`

// The service name carries a per-installation suffix that is not derivable
// from anything on disk, so the bare name is tried first and the suffixed
// one is found by listing the keychain's attributes.
func TestClaudeKeychainFindsSuffixedService(t *testing.T) {
	blob := `{"claudeAiOauth":{"accessToken":"sk-ant-oat-synthetic","expiresAt":4102444800000}}`
	// Only the suffixed service resolves, as on a current install.
	original := securityCommand
	var calls [][]string
	securityCommand = func(ctx context.Context, args ...string) *exec.Cmd {
		calls = append(calls, append([]string(nil), args...))
		out, status := "", "fail"
		switch {
		case args[0] == "dump-keychain":
			out, status = claudeKeychainDump, "ok"
		case reflect.DeepEqual(args, []string{"find-generic-password", "-w", "-s", "Claude Code-credentials-552ffa86"}):
			out, status = blob, "ok"
		}
		cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=TestHelperSecurity")
		cmd.Env = append(os.Environ(),
			"USAGE_HELPER_PROCESS=security",
			"SECURITY_HELPER_STDOUT="+out,
			"SECURITY_HELPER_STATUS="+status,
		)
		return cmd
	}
	t.Cleanup(func() { securityCommand = original })

	token, expiresAt := readClaudeKeychainCredential()

	if token != "sk-ant-oat-synthetic" {
		t.Fatalf("token = %q, want the stored access token", token)
	}
	if expiresAt == nil || expiresAt.UnixMilli() != 4102444800000 {
		t.Errorf("expiresAt = %v, want the stored expiry", expiresAt)
	}
	// The bare service is tried before the keychain is listed at all, so an
	// install using it never pays for a listing.
	want := []string{"find-generic-password", "-w", "-s", claudeKeychainService}
	if len(calls) == 0 || !reflect.DeepEqual(calls[0], want) {
		t.Fatalf("first invocation = %q, want %q", calls, want)
	}
	if len(calls) != 3 || calls[1][0] != "dump-keychain" {
		t.Errorf("invocations = %q, want bare lookup, listing, suffixed lookup", calls)
	}
}

func TestClaudeKeychainServicesListsSuffixedNamesOnce(t *testing.T) {
	fakeSecurity(t, map[string]string{"dump-keychain": claudeKeychainDump + claudeKeychainDump})

	services := suffixedClaudeKeychainServices()

	want := []string{"Claude Code-credentials-552ffa86"}
	if !reflect.DeepEqual(services, want) {
		t.Errorf("services = %q, want %q", services, want)
	}
}

// Listing the keychain is best effort: when it fails, the bare service name
// has already been tried and the answer is simply "not signed in".
func TestClaudeKeychainServicesSurvivesADumpFailure(t *testing.T) {
	fakeSecurity(t, map[string]string{})

	if services := suffixedClaudeKeychainServices(); services != nil {
		t.Errorf("services = %q, want none", services)
	}
}

func TestClaudeKeychainReturnsNothingWhenNoItemMatches(t *testing.T) {
	fakeSecurity(t, map[string]string{"dump-keychain": claudeKeychainDump})

	if token, _ := readClaudeKeychainCredential(); token != "" {
		t.Errorf("token = %q, want empty when no item can be read", token)
	}
}

// An entry holding only MCP server logins carries no Claude subscription
// token; the usage endpoint rejects those, so it must count as "not signed
// in" rather than yielding a token that 401s.
func TestClaudeKeychainBlobIgnoresMCPOnlyEntries(t *testing.T) {
	blob := []byte(`{"mcpOAuth":{"some-server":{"accessToken":"mcp-token"}}}`)

	if token, _, ok := parseClaudeKeychainBlob(blob); ok || token != "" {
		t.Errorf("parse = (%q, %v), want no token", token, ok)
	}
}

func TestClaudeKeychainBlobRejectsUnusableEntries(t *testing.T) {
	for name, blob := range map[string]string{
		"not json":     `not json at all`,
		"empty token":  `{"claudeAiOauth":{"accessToken":""}}`,
		"empty object": `{}`,
	} {
		if _, _, ok := parseClaudeKeychainBlob([]byte(blob)); ok {
			t.Errorf("%s: parse succeeded, want rejection", name)
		}
	}
}

// An entry without an expiry is usable; the endpoint stays the authority.
func TestClaudeKeychainBlobWithoutExpiry(t *testing.T) {
	blob := []byte(`{"claudeAiOauth":{"accessToken":"sk-ant-oat-synthetic","expiresAt":0}}`)

	token, expiresAt, ok := parseClaudeKeychainBlob(blob)
	if !ok || token != "sk-ant-oat-synthetic" {
		t.Fatalf("parse = (%q, %v), want the token", token, ok)
	}
	if expiresAt != nil {
		t.Errorf("expiresAt = %v, want nil", expiresAt)
	}
}

func TestKeychainServiceNameParsesAttributeLines(t *testing.T) {
	line := `    "svce"<blob>="Claude Code-credentials-552ffa86"`
	if name, ok := keychainServiceName(line); !ok || name != "Claude Code-credentials-552ffa86" {
		t.Errorf("keychainServiceName = (%q, %v)", name, ok)
	}
	if _, ok := keychainServiceName(`    "acct"<blob>="dev"`); ok {
		t.Error("expected a non-service attribute line to be ignored")
	}
}

// A stored token past its own expiry is reported as expired rather than as
// signed in, so the 401 that follows is already explained.
func TestClaudeAuthStatusReportsAnExpiredKeychainToken(t *testing.T) {
	expired := time.Now().Add(-time.Hour)
	provider := &ClaudeProvider{oauthToken: "sk-ant-oat-synthetic", oauthSource: "keychain", oauthExpiresAt: &expired}

	status := provider.AuthStatus()
	if status.Status != "expired" {
		t.Errorf("Status = %q, want expired", status.Status)
	}
	if status.Source != "keychain" {
		t.Errorf("Source = %q, want keychain", status.Source)
	}

	valid := time.Now().Add(time.Hour)
	provider.oauthExpiresAt = &valid
	if status := provider.AuthStatus(); status.Status != "available" {
		t.Errorf("Status = %q, want available for an unexpired token", status.Status)
	}
}
