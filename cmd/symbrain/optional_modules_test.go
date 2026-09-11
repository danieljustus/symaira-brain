package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func writeArgRecorder(t *testing.T, dir, name, marker, fakeMCP string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	script := "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"" + marker + "\"\nexec \"" + fakeMCP + "\" \"$@\"\n"
	if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

func optionalProfile(alias string) *profile.Profile {
	return &profile.Profile{Name: "optional", Servers: profile.Servers{
		alias: {Enabled: true, ToolsAllow: []string{"echo"}},
	}}
}

func TestBuildServers_DirectModuleUsesDirectBinaryAndServeArg(t *testing.T) {
	dir := t.TempDir()
	marker := filepath.Join(dir, "argv")
	fakeMCP := buildFakemcpOnce(t)
	writeArgRecorder(t, dir, "symoperate", marker, fakeMCP)
	writeArgRecorder(t, dir, "symcockpit", marker, fakeMCP)
	t.Setenv("PATH", dir)
	t.Setenv("FAKEMCP_TOOLS", `[{"name":"echo","description":"echo","behavior":"echo"}]`)

	var stderr strings.Builder
	servers := buildServers(optionalProfile(profile.ServerOperate), &config.Config{
		Modules: config.ModulesConfig{Operate: true},
	}, &stderr, "")
	ms := servers[profile.ServerOperate]
	if ms == nil {
		t.Fatalf("operate server missing: stderr=%q", stderr.String())
	}
	ctx := context.Background()
	tools, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("tools/list: %v", err)
	}
	if len(tools) != 1 || tools[0].Name != "echo" {
		t.Fatalf("tools/list = %#v, want echo", tools)
	}
	call, err := ms.CallTool(ctx, "echo", json.RawMessage(`{"value":"ok"}`))
	if err != nil {
		t.Fatalf("tools/call: %v", err)
	}
	if call.IsError {
		t.Fatalf("tools/call returned tool error: %#v", call)
	}
	ms.Shutdown()
	args, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("read argv marker: %v", err)
	}
	if got := strings.TrimSpace(string(args)); got != "serve" {
		t.Fatalf("direct argv = %q, want %q", got, "serve")
	}
}

func TestBuildServers_MissingDirectFallsBackToCockpitSubcommand(t *testing.T) {
	dir := t.TempDir()
	marker := filepath.Join(dir, "argv")
	fakeMCP := buildFakemcpOnce(t)
	writeArgRecorder(t, dir, "symcockpit", marker, fakeMCP)
	t.Setenv("PATH", dir)

	var stderr strings.Builder
	servers := buildServers(optionalProfile(profile.ServerScope), &config.Config{
		Modules: config.ModulesConfig{Scope: true},
	}, &stderr, "")
	ms := servers[profile.ServerScope]
	if ms == nil {
		t.Fatalf("scope fallback missing: stderr=%q", stderr.String())
	}
	if _, err := ms.ListTools(context.Background()); err != nil {
		t.Fatalf("fallback tools/list: %v", err)
	}
	ms.Shutdown()
	args, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("read argv marker: %v", err)
	}
	if got := strings.TrimSpace(string(args)); got != "scope serve" {
		t.Fatalf("fallback argv = %q, want %q", got, "scope serve")
	}
}

func TestBuildServers_InvalidExplicitOverrideDoesNotFallback(t *testing.T) {
	dir := t.TempDir()
	marker := filepath.Join(dir, "argv")
	fakeMCP := buildFakemcpOnce(t)
	writeArgRecorder(t, dir, "symcockpit", marker, fakeMCP)
	t.Setenv("PATH", dir)

	var stderr strings.Builder
	servers := buildServers(optionalProfile(profile.ServerOperate), &config.Config{
		Modules: config.ModulesConfig{Operate: true},
		Servers: config.ServersConfig{Operate: config.ServerOverride{BinaryPath: filepath.Join(dir, "missing")}},
	}, &stderr, "")
	if len(servers) != 0 {
		t.Fatalf("invalid explicit override registered fallback: %v", servers)
	}
	if !strings.Contains(stderr.String(), "invalid explicit binary override") {
		t.Fatalf("stderr = %q, want explicit override error", stderr.String())
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatalf("fallback was probed/spawned, marker stat error = %v", err)
	}
}
