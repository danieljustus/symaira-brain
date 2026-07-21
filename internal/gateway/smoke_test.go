//go:build smoke

package gateway

import (
	"context"
	"os/exec"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// TestSmoke_RealCorePath runs an end-to-end test against real Symaira
// binaries when available. It skips cleanly in CI or when the binaries
// are not installed.
//
// Run with: go test -tags smoke ./internal/gateway/ -run TestSmoke -v
func TestSmoke_RealCorePath(t *testing.T) {
	if testing.Short() {
		t.Skip("smoke tests skipped in -short mode")
	}

	servers := []struct {
		name   string
		binary string
	}{
		{"vault", "symvault"},
		{"memory", "symmemory"},
		{"skills", "symskills"},
	}

	var available []string
	for _, s := range servers {
		if _, err := exec.LookPath(s.binary); err != nil {
			t.Logf("%s not on PATH: %v — skipping", s.binary, err)
			continue
		}
		available = append(available, s.name)
	}

	if len(available) == 0 {
		t.Skip("no Symaira core binaries on PATH — skipping smoke test")
	}

	t.Logf("available cores: %v", available)

	p := &profile.Profile{
		Name: "personal",
		Servers: profile.Servers{
			Vault:  profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull},
			Memory: profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite},
			Skills: profile.ServerConfig{Enabled: true},
		},
	}

	for _, s := range servers {
		if !contains(available, s.name) {
			continue
		}

		t.Run(s.name, func(t *testing.T) {
			ms := broker.NewManagedServer(broker.ServerConfig{
				Name:        s.name,
				BinaryPath:  s.binary,
				MaxRestarts: 0,
			})
			defer ms.Shutdown()

			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()

			tools, err := ms.ListTools(ctx)
			if err != nil {
				t.Fatalf("ListTools(%s): %v", s.name, err)
			}

			t.Logf("%s: %d tools reported", s.name, len(tools))
			if len(tools) == 0 {
				t.Errorf("%s reported zero tools", s.name)
			}

			serverCfg := p.Server(s.name)
			liveNames := make([]string, len(tools))
			for i, tool := range tools {
				liveNames[i] = tool.Name
			}

			report, err := policy.Evaluate(s.name, serverCfg, liveNames)
			if err != nil {
				t.Fatalf("policy.Evaluate(%s): %v", s.name, err)
			}

			t.Logf("%s policy: exposed=%d hidden=%d unknown=%d",
				s.name, len(report.Exposed), len(report.Hidden), len(report.Unknown))

			catTools := make([]catalog.Tool, len(tools))
			for i, tool := range tools {
				catTools[i] = catalog.Tool{
					Name:        tool.Name,
					Description: tool.Description,
					InputSchema: tool.InputSchema,
				}
			}

			cat, err := catalog.Build([]catalog.ServerTools{
				{Server: s.name, Tools: catTools, Report: report},
			})
			if err != nil {
				t.Fatalf("catalog.Build(%s): %v", s.name, err)
			}

			exposed := cat.Exposed()
			t.Logf("%s catalog: %d exposed tools", s.name, len(exposed))
		})
	}
}

// TestSmoke_VaultSessionBehavior documents how vault session/unlock
// behaves when spawned as a child per gateway process. This is the
// biggest UX unknown before beta (issue #17).
//
// Findings:
//   - symvault requires interactive unlock (Touch ID / master password)
//     on first access. When spawned as a child process, the stdin/stdout
//     are captured by the MCP transport, so the unlock prompt goes to
//     the child's stderr.
//   - The child process inherits the parent's TTY if available (os.Stderr
//     is connected to the terminal). This means Touch ID prompts appear
//     on the user's terminal.
//   - If the vault is already unlocked (session exists), the child
//     responds immediately without prompting.
//   - For non-interactive environments (CI, headless), the vault must
//     be pre-unlocked or the child will hang on the unlock prompt.
func TestSmoke_VaultSessionBehavior(t *testing.T) {
	if testing.Short() {
		t.Skip("smoke tests skipped in -short mode")
	}

	if _, err := exec.LookPath("symvault"); err != nil {
		t.Skip("symvault not on PATH — skipping vault session test")
	}

	ms := broker.NewManagedServer(broker.ServerConfig{
		Name:        "vault",
		BinaryPath:  "symvault",
		MaxRestarts: 0,
	})
	defer ms.Shutdown()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	tools, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools(vault): %v", err)
	}

	t.Logf("vault tools: %d reported", len(tools))
	for _, tool := range tools {
		t.Logf("  - %s: %s", tool.Name, tool.Description)
	}

	t.Log("Vault session behavior findings:")
	t.Log("  - vault uses interactive unlock (Touch ID / master password)")
	t.Log("  - unlock prompt appears on child's stderr (terminal if connected)")
	t.Log("  - pre-unlocked vault responds immediately without prompting")
	t.Log("  - non-interactive environments need pre-unlocked vault")
}

func contains(slice []string, item string) bool {
	for _, s := range slice {
		if s == item {
			return true
		}
	}
	return false
}
