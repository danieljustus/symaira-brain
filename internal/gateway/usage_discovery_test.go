//go:build darwin || linux

package gateway

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/usage"
)

func TestUsageDiscoveryIsDeferredAndRepeatedPerCall(t *testing.T) {
	root := t.TempDir()
	home := filepath.Join(root, "home")
	bin := filepath.Join(root, "bin")
	if err := os.MkdirAll(home, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(bin, 0o700); err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(root, "ps-invocations")
	ps := filepath.Join(bin, "ps")
	if err := os.WriteFile(ps, []byte("#!/bin/sh\nprintf x >> \"$SYMBRAIN_TEST_PS_MARKER\"\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("SYMBRAIN_TEST_PS_MARKER", marker)
	for _, name := range []string{
		"ANTHROPIC_ADMIN_KEY", "COPILOT_ACCESS_TOKEN", "CODEX_ACCESS_TOKEN", "CURSOR_COOKIE",
		"KIMI_CODE_API_KEY", "KIMI_AUTH_TOKEN", "MOONSHOT_API_KEY", "NOUS_PORTAL_ACCESS_TOKEN",
		"OPENCODE_COOKIE", "OPENROUTER_API_KEY", "CODEX_HOME", "KIMI_CODE_HOME", "HERMES_HOME",
	} {
		t.Setenv(name, "")
	}
	t.Setenv("ANTHROPIC_OAUTH_TOKEN", "synthetic-no-keychain-token")

	server := New(&profile.Profile{}, nil, nil, nil, "test")
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatalf("server construction ran process discovery: stat marker error = %v", err)
	}

	first := handleUsageWithCancelledContext(t, server)
	assertCodexUsage(t, first, false, "")
	assertProbeCount(t, marker, 1)

	codexDir := filepath.Join(home, ".codex")
	if err := os.MkdirAll(codexDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(codexDir, "auth.json"), []byte(`{"tokens":{"access_token":"synthetic-codex-file-token"}}`), 0o600); err != nil {
		t.Fatal(err)
	}

	second := handleUsageWithCancelledContext(t, server)
	assertCodexUsage(t, second, true, "file")
	assertProbeCount(t, marker, 2)
}

func handleUsageWithCancelledContext(t *testing.T, server *Server) usage.Report {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	result, err := server.handleAIUsage(ctx, json.RawMessage(`{}`))
	if err != nil {
		t.Fatalf("handle usage: %v", err)
	}
	report, ok := result.(usage.Report)
	if !ok {
		t.Fatalf("usage result type = %T, want usage.Report", result)
	}
	return report
}

func assertCodexUsage(t *testing.T, report usage.Report, configured bool, source string) {
	t.Helper()
	for _, provider := range report.Providers {
		if provider.ID == "codex" {
			if provider.Configured != configured || provider.AuthStatus.Source != source {
				t.Fatalf("Codex row = configured %v, source %q; want configured %v, source %q", provider.Configured, provider.AuthStatus.Source, configured, source)
			}
			return
		}
	}
	t.Fatal("report omitted Codex")
}

func assertProbeCount(t *testing.T, marker string, want int) {
	t.Helper()
	data, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("read process-probe marker: %v", err)
	}
	if got := len(strings.TrimSpace(string(data))); got != want {
		t.Fatalf("process probe calls = %d, want %d", got, want)
	}
}
