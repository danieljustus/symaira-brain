package usage

import (
	"context"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

type referenceTransport struct {
	t     *testing.T
	calls int
}

func (r *referenceTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	r.calls++
	if req.Header.Get("Authorization") != "Bearer fixture-secret" {
		r.t.Error("resolved credential not used")
	}
	return &http.Response{StatusCode: 200, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(`{"data":{"total_credits":10,"total_usage":1}}`)), Request: req}, nil
}

func TestAllProviderReferenceFailures(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{"HOME", "USERPROFILE", "CODEX_HOME", "HERMES_HOME", "KIMI_CODE_HOME", "GH_CONFIG_DIR", "XDG_CONFIG_HOME"} {
		t.Setenv(name, dir)
	}
	t.Setenv("PATH", dir) // No executable can reach the operator's vault or keychain.
	names := []string{"ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN", "CODEX_ACCESS_TOKEN", "COPILOT_ACCESS_TOKEN", "CURSOR_COOKIE", "KIMI_CODE_API_KEY", "KIMI_AUTH_TOKEN", "MOONSHOT_API_KEY", "NOUS_PORTAL_ACCESS_TOKEN", "OPENCODE_COOKIE", "OPENROUTER_API_KEY"}
	for _, name := range names {
		t.Setenv(name, "")
	}
	factories := []func(*http.Client) Provider{
		func(c *http.Client) Provider {
			return newClaudeProvider(c, func() (string, *time.Time) { return "", nil })
		},
		func(c *http.Client) Provider {
			return newClaudeProvider(c, func() (string, *time.Time) { return "", nil })
		},
		func(c *http.Client) Provider { return NewCodexProvider(c) },
		func(c *http.Client) Provider { return NewCopilotProvider(c) },
		func(c *http.Client) Provider { return NewCursorProvider(c) },
		func(c *http.Client) Provider { return NewKimiProvider(c) },
		func(c *http.Client) Provider { return NewKimiProvider(c) },
		func(c *http.Client) Provider { return NewMoonshotProvider(c) },
		func(c *http.Client) Provider { return NewNousPortalProvider(c) },
		func(c *http.Client) Provider { return NewOpenCodeProvider(c) },
		func(c *http.Client) Provider { return NewOpenRouterProvider(c) },
	}
	for i, name := range names {
		t.Run(name, func(t *testing.T) {
			t.Setenv(name, "symvault://fixture/provider")
			transport := &referenceTransport{t: t}
			provider := factories[i](&http.Client{Transport: transport})
			got := BuildReport(context.Background(), []Provider{provider}).Providers[0]
			if !got.Configured || !strings.Contains(got.Error, "symvault://fixture/provider") || !strings.Contains(got.Error, "credential resolution failed:") {
				t.Fatalf("reference failure lost: %+v", got)
			}
			if transport.calls != 0 {
				t.Fatal("failed reference started a request")
			}
		})
	}
}

func TestReferenceReport(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fixture executable uses /bin/sh")
	}
	for _, scheme := range []string{"symvault://", "vault://"} {
		for _, mode := range []string{"ok", "missing", "locked", "store", "binary"} {
			t.Run(scheme+mode, func(t *testing.T) {
				dir := t.TempDir()
				t.Setenv("PATH", dir)
				t.Setenv("OPENROUTER_API_KEY", scheme+"fixture/openrouter")
				if mode != "binary" {
					script := "#!/bin/sh\n[ \"$1\" = get ] && [ \"$2\" = -- ] && [ \"$3\" = fixture/openrouter ] && [ \"$4\" = --print ] || exit 9\n"
					if mode == "ok" {
						script += "printf 'fixture-secret\\n'\n"
					} else {
						script += "printf 'fixture-secret\\n'; printf '" + mode + "\\n' >&2; exit 1\n"
					}
					if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0700); err != nil {
						t.Fatal(err)
					}
				}
				transport := &referenceTransport{t: t}
				report := BuildReport(context.Background(), []Provider{NewOpenRouterProvider(&http.Client{Transport: transport})})
				got := report.Providers[0]
				if !got.Configured {
					t.Error("reference must count as configured")
				}
				if mode == "ok" {
					if got.Error != "" || got.Snapshot == nil || transport.calls != 2 {
						t.Fatalf("successful reference not fetched: %+v, calls=%d", got, transport.calls)
					}
				} else {
					if !strings.Contains(got.Error, "credential resolution failed:") || !strings.Contains(got.Error, "symvault://fixture/openrouter") {
						t.Errorf("missing reference error: %q", got.Error)
					}
					if strings.Contains(got.Error, "fixture-secret") || transport.calls != 0 {
						t.Error("failure leaked a secret or fetched usage")
					}
				}
			})
		}
	}
}
