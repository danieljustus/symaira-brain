package usage

import (
	"os"
	"path/filepath"
	"testing"
)

// withVault installs a fake vaultResolver for the test and restores the
// production resolver afterwards.
func withVault(t *testing.T, responses map[string]string) {
	t.Helper()
	old := vaultResolver
	vaultResolver = fakeVault(responses)
	t.Cleanup(func() { vaultResolver = old })
}

// TestProvidersResolveSymvaultURIs verifies that every provider whose
// primary credential comes from an env var accepts a symvault:// URI and
// reports the resolved credential with Source "vault".
func TestProvidersResolveSymvaultURIs(t *testing.T) {
	withVault(t, map[string]string{
		"symvault://ai/openrouter/key":  "sk-openrouter",
		"symvault://ai/moonshot/key":    "sk-moonshot",
		"symvault://ai/kimi/key":        "sk-kimi",
		"symvault://ai/codex/token":     "tok-codex",
		"symvault://ai/copilot/token":   "tok-copilot",
		"symvault://ai/nous/token":      "tok-nous",
		"symvault://ai/claude/oauth":    "tok-claude",
		"symvault://ai/cursor/cookie":   "tok-cursor",
		"symvault://ai/opencode/cookie": "tok-opencode",
	})

	// Keep HOME away from any real auth files during the test.
	t.Setenv("HERMES_HOME", t.TempDir())
	t.Setenv("CODEX_HOME", t.TempDir())
	t.Setenv("KIMI_CODE_HOME", t.TempDir())
	t.Setenv("HOME", t.TempDir())

	cases := []struct {
		name  string
		env   string
		uri   string
		build func() Provider
	}{
		{"openrouter", "OPENROUTER_API_KEY", "symvault://ai/openrouter/key", func() Provider { return NewOpenRouterProvider(nil) }},
		{"moonshot", "MOONSHOT_API_KEY", "symvault://ai/moonshot/key", func() Provider { return NewMoonshotProvider(nil) }},
		{"kimi", "KIMI_CODE_API_KEY", "symvault://ai/kimi/key", func() Provider { return NewKimiProvider(nil) }},
		{"codex", "CODEX_ACCESS_TOKEN", "symvault://ai/codex/token", func() Provider { return NewCodexProvider(nil) }},
		{"copilot", "COPILOT_ACCESS_TOKEN", "symvault://ai/copilot/token", func() Provider { return NewCopilotProvider(nil) }},
		{"nous", "NOUS_PORTAL_ACCESS_TOKEN", "symvault://ai/nous/token", func() Provider { return NewNousPortalProvider(nil) }},
		{"claude", "ANTHROPIC_OAUTH_TOKEN", "symvault://ai/claude/oauth", func() Provider { return NewClaudeProvider(nil) }},
		{"cursor", "CURSOR_COOKIE", "symvault://ai/cursor/cookie", func() Provider { return NewCursorProvider(nil) }},
		{"opencode", "OPENCODE_COOKIE", "symvault://ai/opencode/cookie", func() Provider { return NewOpenCodeProvider(nil) }},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv(tc.env, tc.uri)
			p := tc.build()
			if !p.IsConfigured() {
				t.Fatalf("%s: IsConfigured = false with symvault URI set", tc.name)
			}
			as := p.AuthStatus()
			if as.Status != "available" {
				t.Fatalf("%s: AuthStatus = %+v, want available", tc.name, as)
			}
			if as.Source != "vault" {
				t.Fatalf("%s: AuthStatus.Source = %q, want \"vault\"", tc.name, as.Source)
			}
			if len(p.Strategies()) == 0 {
				t.Fatalf("%s: no strategies despite configured credential", tc.name)
			}
		})
	}
}

// TestProvidersVaultFailureReportsMissing verifies that a failing
// symvault:// resolution reports the provider as unconfigured with an
// informative AuthStatus, and never falls back to a file silently.
func TestProvidersVaultFailureReportsMissing(t *testing.T) {
	withVault(t, nil) // every lookup fails

	t.Setenv("HERMES_HOME", t.TempDir())
	t.Setenv("CODEX_HOME", t.TempDir())
	t.Setenv("HOME", t.TempDir())

	cases := []struct {
		name  string
		env   string
		build func() Provider
	}{
		{"openrouter", "OPENROUTER_API_KEY", func() Provider { return NewOpenRouterProvider(nil) }},
		{"codex", "CODEX_ACCESS_TOKEN", func() Provider { return NewCodexProvider(nil) }},
		{"copilot", "COPILOT_ACCESS_TOKEN", func() Provider { return NewCopilotProvider(nil) }},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv(tc.env, "symvault://ai/missing/"+tc.name)
			p := tc.build()
			if p.IsConfigured() {
				t.Fatalf("%s: IsConfigured = true despite failing vault lookup", tc.name)
			}
			as := p.AuthStatus()
			if as.Status != "missing" {
				t.Fatalf("%s: AuthStatus = %+v, want missing", tc.name, as)
			}
			if as.Source != "vault" {
				t.Fatalf("%s: AuthStatus.Source = %q, want vault", tc.name, as.Source)
			}
			if as.Detail == "" {
				t.Fatalf("%s: AuthStatus.Detail is empty", tc.name)
			}
			if len(p.Strategies()) != 0 {
				t.Fatalf("%s: strategies present despite failing credential", tc.name)
			}
		})
	}
}

// TestFileCredentialFallbackStillWorks guards the unchanged behavior: with
// no env var set, the read-only CLI file fallback still configures the
// provider (regression guard for the ported credential chain).
func TestFileCredentialFallbackStillWorks(t *testing.T) {
	withVault(t, nil)

	home := t.TempDir()
	t.Setenv("HERMES_HOME", home)
	t.Setenv("HOME", home)

	// Codex auth file
	codexHome := home
	if err := os.MkdirAll(codexHome, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(codexHome, "auth.json"),
		[]byte(`{"access_token":"file-token-codex"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("CODEX_HOME", codexHome)
	t.Setenv("CODEX_ACCESS_TOKEN", "")

	p := NewCodexProvider(nil)
	if !p.IsConfigured() {
		t.Fatal("codex: file fallback did not configure the provider")
	}
	if as := p.AuthStatus(); as.Source != "file" {
		t.Fatalf("codex: AuthStatus.Source = %q, want file", as.Source)
	}
}
