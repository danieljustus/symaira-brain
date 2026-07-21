package gateway

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

var fakeBin string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "symbrain-gateway-test-*")
	if err != nil {
		panic("gateway test: mkdtemp: " + err.Error())
	}
	defer os.RemoveAll(dir)

	fakeBin = filepath.Join(dir, "fakemcp")
	cmd := exec.Command("go", "build", "-o", fakeBin, "../broker/testdata/fakemcp")
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if out, err := cmd.CombinedOutput(); err != nil {
		panic("gateway test: build fakemcp: " + err.Error() + "\n" + string(out))
	}

	os.Exit(m.Run())
}

func spawnFake(t *testing.T, env map[string]string) *broker.Client {
	t.Helper()
	var fullEnv []string
	for k, v := range env {
		fullEnv = append(fullEnv, k+"="+v)
	}

	c, err := broker.Spawn(fakeBin, broker.Options{
		Env:    fullEnv,
		Stderr: testStderr{t},
	})
	if err != nil {
		t.Fatalf("Spawn(fakemcp): %v", err)
	}
	t.Cleanup(func() {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize: %v", err)
	}
	return c
}

type testStderr struct{ t *testing.T }

func (w testStderr) Write(p []byte) (int, error) {
	w.t.Logf("fakemcp stderr: %s", p)
	return len(p), nil
}

func TestGateway_CatalogBuild(t *testing.T) {
	vaultClient := spawnFake(t, map[string]string{
		"FAKEMCP_TOOLS": `[{"name":"get_entry","description":"fetch"},{"name":"health","description":"hc"}]`,
	})
	memoryClient := spawnFake(t, map[string]string{
		"FAKEMCP_TOOLS": `[{"name":"memory_search","description":"search"},{"name":"entity_list","description":"list"}]`,
	})

	vaultTools, _ := vaultClient.ListTools(context.Background())
	memoryTools, _ := memoryClient.ListTools(context.Background())

	vaultCfg := profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull}
	vaultLive := make([]string, len(vaultTools))
	for i, t := range vaultTools {
		vaultLive[i] = t.Name
	}
	vaultReport, err := policy.Evaluate("vault", vaultCfg, vaultLive)
	if err != nil {
		t.Fatalf("policy.Evaluate(vault): %v", err)
	}

	memCfg := profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite}
	memLive := make([]string, len(memoryTools))
	for i, t := range memoryTools {
		memLive[i] = t.Name
	}
	memReport, err := policy.Evaluate("memory", memCfg, memLive)
	if err != nil {
		t.Fatalf("policy.Evaluate(memory): %v", err)
	}

	cat, err := catalog.Build([]catalog.ServerTools{
		{Server: "vault", Tools: toCatalogTools(vaultTools), Report: vaultReport},
		{Server: "memory", Tools: toCatalogTools(memoryTools), Report: memReport},
	})
	if err != nil {
		t.Fatalf("catalog.Build: %v", err)
	}

	exposed := cat.Exposed()
	names := make(map[string]bool)
	for _, e := range exposed {
		names[e.Name] = true
	}

	if !names["vault_get_entry"] {
		t.Error("missing vault_get_entry")
	}
	if !names["vault_health"] {
		t.Error("missing vault_health")
	}
	if !names["memory_search"] {
		t.Error("missing memory_search")
	}
	if !names["entity_list"] {
		t.Error("missing entity_list")
	}
}

func TestGateway_PolicyRequestOnly(t *testing.T) {
	client := spawnFake(t, map[string]string{
		"FAKEMCP_TOOLS": `[{"name":"get_entry","description":"fetch"},{"name":"health","description":"hc"},{"name":"request_credential","description":"req"}]`,
	})

	tools, _ := client.ListTools(context.Background())
	live := make([]string, len(tools))
	for i, tool := range tools {
		live[i] = tool.Name
	}

	report, err := policy.Evaluate("vault", profile.ServerConfig{
		Enabled: true,
		Mode:    profile.VaultModeRequestOnly,
	}, live)
	if err != nil {
		t.Fatalf("policy.Evaluate: %v", err)
	}

	hidden := make(map[string]bool)
	for _, name := range report.Hidden {
		hidden[name] = true
	}
	exposed := make(map[string]bool)
	for _, name := range report.Exposed {
		exposed[name] = true
	}

	if !hidden["get_entry"] {
		t.Error("get_entry should be hidden in request_only")
	}
	if !exposed["health"] {
		t.Error("health should be exposed in request_only")
	}
	if !exposed["request_credential"] {
		t.Error("request_credential should be exposed in request_only")
	}
}

func TestGateway_SkillsDisabled(t *testing.T) {
	p := &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			Vault:  profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull},
			Memory: profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite},
			Skills: profile.ServerConfig{Enabled: false},
		},
	}

	if p.Servers.Skills.Enabled {
		t.Error("skills should be disabled")
	}
}

func TestGateway_CallToolRoundTrip(t *testing.T) {
	client := spawnFake(t, map[string]string{
		"FAKEMCP_TOOLS": `[{"name":"get_entry","description":"fetch"}]`,
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	result, err := client.CallTool(ctx, "get_entry", json.RawMessage(`{"id":"test"}`))
	if err != nil {
		t.Fatalf("CallTool: %v", err)
	}
	if result.IsError {
		t.Fatalf("IsError = true")
	}
	if len(result.Content) != 1 {
		t.Fatalf("content len = %d, want 1", len(result.Content))
	}
}

func toCatalogTools(tools []broker.Tool) []catalog.Tool {
	result := make([]catalog.Tool, len(tools))
	for i, t := range tools {
		result[i] = catalog.Tool{
			Name:        t.Name,
			Description: t.Description,
			InputSchema: t.InputSchema,
		}
	}
	return result
}
