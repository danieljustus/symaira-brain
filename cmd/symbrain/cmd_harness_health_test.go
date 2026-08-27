package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// buildFakeMCP compiles the broker's fakemcp fixture so cmd-level harness
// health tests can probe a real stdio MCP server without a Symaira binary.
func buildFakeMCP(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	bin := filepath.Join(dir, "fakemcp")
	cmd := exec.Command("go", "build", "-o", bin, "../../internal/broker/testdata/fakemcp")
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("build fakemcp: %v\n%s", err, out)
	}
	return bin
}

// writeHealthConfig writes a JSON harness config whose server map points
// one entry at fakemcp (optionally slow to initialize) and one at a URL.
func writeHealthConfig(t *testing.T, fakeBin string, initDelayMS string) {
	t.Helper()
	h := harnessByName(t, "claude")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath(): %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	env := "{}"
	if initDelayMS != "" {
		// Env *values* in a harness config are never read back or replayed
		// into the spawned child — only their names are reported. This
		// block exercises that path.
		env = `{"FAKEMCP_INIT_DELAY_MS":"` + initDelayMS + `"}`
	}
	data := []byte(`{"mcpServers":{
  "good": {"command": "` + fakeBin + `", "env": ` + env + `},
  "remote": {"url": "http://localhost:9000/mcp"}
}}`)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}
}

// runHarnessHealth invokes `symbrain harness health [args...] --json` and
// decodes the report.
func runHarnessHealth(t *testing.T, args ...string) (harnessHealthReport, exitcodes.ExitCode, string) {
	t.Helper()
	var stdout, stderr bytes.Buffer
	full := append([]string{"harness", "health"}, args...)
	full = append(full, "--json")
	code := run(full, &stdout, &stderr)
	var report harnessHealthReport
	if code == exitcodes.ExitOK {
		if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
			t.Fatalf("decode health report: %v (%q)", err, stdout.String())
		}
	}
	return report, code, stderr.String()
}

func TestHarnessHealth_HealthyAndURLSkipped(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	fake := buildFakeMCP(t)
	writeHealthConfig(t, fake, "")

	report, code, _ := runHarnessHealth(t)
	if code != exitcodes.ExitOK {
		t.Fatalf("run() = %d, want 0", code)
	}
	if len(report.Servers) != 2 {
		t.Fatalf("servers = %d, want 2 (good + remote)", len(report.Servers))
	}
	byName := map[string]harnessHealthEntry{}
	for _, s := range report.Servers {
		byName[s.Server] = s
	}
	good := byName["good"]
	if !good.Healthy {
		t.Errorf("good server = %+v, want healthy (fakemcp answers initialize)", good)
	}
	if good.Error != "" {
		t.Errorf("good server error = %q, want empty", good.Error)
	}
	remote := byName["remote"]
	if remote.Healthy {
		t.Error("remote url server reported healthy, want skipped")
	}
	if remote.Transport != "http" || !strings.Contains(remote.Error, "not stdio") {
		t.Errorf("remote server = %+v, want http transport + not-stdio skip", remote)
	}
}

func TestHarnessHealth_WedgedDoesNotBlock(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	// Spawned children inherit the test env; a 6s init delay wedges the
	// probe (bound 5s) — the run must complete fast and report the server
	// unhealthy instead of blocking the whole report.
	t.Setenv("FAKEMCP_INIT_DELAY_MS", "6000")
	fake := buildFakeMCP(t)
	writeHealthConfig(t, fake, "")

	report, code, _ := runHarnessHealth(t)
	if code != exitcodes.ExitOK {
		t.Fatalf("run() = %d, want 0", code)
	}
	if len(report.Servers) != 2 {
		t.Fatalf("servers = %d, want 2 (wedged + remote)", len(report.Servers))
	}
	byName := map[string]harnessHealthEntry{}
	for _, s := range report.Servers {
		byName[s.Server] = s
	}
	wedged := byName["good"]
	if wedged.Healthy {
		t.Error("wedged server reported healthy, want unhealthy")
	}
	if !strings.Contains(wedged.Error, "initialize") && !strings.Contains(wedged.Error, "timeout") {
		t.Errorf("wedged server error = %q, want initialize/timeout failure", wedged.Error)
	}
	// The URL server is reported independently — no probe stalls it.
	if remote := byName["remote"]; remote.Healthy {
		t.Error("remote url server reported healthy, want skipped")
	}
}

func TestHarnessHealth_HarnessFilter(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	fake := buildFakeMCP(t)
	writeHealthConfig(t, fake, "")

	report, code, _ := runHarnessHealth(t, "--harness", "cursor")
	if code != exitcodes.ExitOK {
		t.Fatalf("run() = %d, want 0", code)
	}
	// The config was written for claude, so cursor has no config -> no servers.
	if len(report.Servers) != 0 {
		t.Fatalf("servers = %d, want 0 for unconfigured harness", len(report.Servers))
	}
}
