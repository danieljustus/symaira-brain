package main

import (
	"bytes"
	"encoding/json"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// envKeysForProviders lists every credential env var the usage providers
// read; the test clears them all so the report is deterministic (every
// provider unconfigured, no network access) regardless of the runner's
// environment. File-based fallbacks are neutralized by pointing the
// provider home dirs at a fresh temp dir.
var envKeysForProviders = []string{
	"OPENROUTER_API_KEY",
	"MOONSHOT_API_KEY",
	"KIMI_CODE_API_KEY",
	"KIMI_AUTH_TOKEN",
	"ANTHROPIC_ADMIN_KEY",
	"ANTHROPIC_OAUTH_TOKEN",
	"CODEX_ACCESS_TOKEN",
	"COPILOT_ACCESS_TOKEN",
	"NOUS_PORTAL_ACCESS_TOKEN",
	"CURSOR_COOKIE",
	"OPENCODE_COOKIE",
}

// TestCmdUsageJSONSchema pins the acceptance contract from issue #290:
// `symbrain usage --output json` returns the schema_version 1 report with
// one entry per ported provider, every provider carrying the
// configured/auth-status fields.
func TestCmdUsageJSONSchema(t *testing.T) {
	for _, k := range envKeysForProviders {
		t.Setenv(k, "")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("CODEX_HOME", home)
	t.Setenv("HERMES_HOME", home)
	t.Setenv("KIMI_CODE_HOME", home)

	var stdout, stderr bytes.Buffer
	code := cmdUsageWithFormat(nil, &stdout, &stderr, output.FormatJSON)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdUsageWithFormat = %d, stderr: %s", code, stderr.String())
	}

	var report struct {
		SchemaVersion int `json:"schema_version"`
		Providers     []struct {
			ID          string `json:"id"`
			DisplayName string `json:"display_name"`
			Configured  bool   `json:"configured"`
			AuthStatus  struct {
				Status string `json:"status"`
			} `json:"auth_status"`
		} `json:"providers"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("output is not valid JSON: %v\n%s", err, stdout.String())
	}
	if report.SchemaVersion != 1 {
		t.Errorf("schema_version = %d, want 1", report.SchemaVersion)
	}
	want := []string{"claude", "codex", "copilot", "cursor", "kimi", "moonshot", "nous", "opencode", "openrouter", "antigravity"}
	if len(report.Providers) != len(want) {
		t.Fatalf("providers = %d entries, want %d", len(report.Providers), len(want))
	}
	seen := map[string]bool{}
	for _, p := range report.Providers {
		seen[p.ID] = true
		if p.DisplayName == "" {
			t.Errorf("provider %s: empty display_name", p.ID)
		}
		if p.AuthStatus.Status == "" {
			t.Errorf("provider %s: empty auth_status.status", p.ID)
		}
		// No provider may read a credential from anywhere but the env vars
		// above or an explicit vault URI — with all of them cleared and
		// home dirs fresh, every non-file provider must be unconfigured.
		// (antigravity is deliberately always-configured: it probes a
		// running process, it has no credential.)
		if p.ID != "antigravity" && p.Configured {
			t.Errorf("provider %s: configured despite cleared env and fresh home", p.ID)
		}
	}
	for _, id := range want {
		if !seen[id] {
			t.Errorf("provider %s missing from report", id)
		}
	}
}

// TestCmdUsageTableOutput pins the human-readable table path: it must
// render without error and name every provider.
func TestCmdUsageTableOutput(t *testing.T) {
	for _, k := range envKeysForProviders {
		t.Setenv(k, "")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("CODEX_HOME", home)
	t.Setenv("HERMES_HOME", home)
	t.Setenv("KIMI_CODE_HOME", home)

	var stdout, stderr bytes.Buffer
	code := cmdUsageWithFormat(nil, &stdout, &stderr, output.FormatTable)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdUsageWithFormat = %d, stderr: %s", code, stderr.String())
	}
	for _, id := range []string{"claude", "codex", "copilot", "cursor", "kimi", "moonshot", "nous", "opencode", "openrouter", "antigravity"} {
		if !bytes.Contains(stdout.Bytes(), []byte(id)) {
			t.Errorf("table output missing provider %q:\n%s", id, stdout.String())
		}
	}
}
