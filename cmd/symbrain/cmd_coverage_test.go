package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// These tests drive the command entry points directly (in-process) so the
// coverage tooling sees the full command bodies, not just the dispatch
// table. The MCP gateway test reuses the fake-MCP child pattern proven by
// hygiene_test.go, but runs cmdServe in-process by swapping os.Stdin and
// os.Stdout.

// ---- serve ----

func TestCmdServe_RequiresProfile(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdServe(nil, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("serve without --profile = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "--profile is required") {
		t.Errorf("stderr = %q, want missing-profile hint", stderr.String())
	}
}

func TestCmdServe_UnknownProfile(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdServe([]string{"--profile", "ghost"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("serve with unknown profile = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "ghost") {
		t.Errorf("stderr = %q, want it to name the missing profile", stderr.String())
	}
}

// TestCmdServe_StdioHandshake runs the real MCP gateway in-process: a
// profile backed by fake-MCP children, a JSON-RPC initialize round-trip,
// then stdin close -> clean exit. This exercises cmdServe and buildServers
// (spawn, catalog, handshake, routing, shutdown) with coverage attribution.
func TestCmdServe_StdioHandshake(t *testing.T) {
	home := sandboxHome(t)
	fakeMCP := buildFakemcpOnce(t)

	vaultWrapper := writeFakeWrapper(t, home, "fakevault",
		`[{"name":"get_entry","description":"fetch"}]`, fakeMCP)
	memoryWrapper := writeFakeWrapper(t, home, "fakememory",
		`[{"name":"memory_search","description":"search"}]`, fakeMCP)

	writeProfileFile(t, home, "stdio-test", `[profile]
name = "stdio-test"

[servers.vault]
enabled = true
mode = "full"

[servers.memory]
enabled = true
mode = "read_write"

[servers.skills]
enabled = false

[audit]
enabled = false
`)
	t.Setenv("SYMBRAIN_SERVERS_VAULT_BINARY_PATH", vaultWrapper)
	t.Setenv("SYMBRAIN_SERVERS_MEMORY_BINARY_PATH", memoryWrapper)

	stdinR, stdinW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	stdoutR, stdoutW, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}

	oldStdin, oldStdout := os.Stdin, os.Stdout
	os.Stdin, os.Stdout = stdinR, stdoutW
	defer func() {
		os.Stdin, os.Stdout = oldStdin, oldStdout
		stdinR.Close()
		stdoutW.Close()
	}()

	var stderr bytes.Buffer
	codeCh := make(chan exitcodes.ExitCode, 1)
	go func() {
		codeCh <- cmdServe([]string{"--profile", "stdio-test"}, &stderr, &stderr)
	}()

	frames := make(chan json.RawMessage)
	scanErr := make(chan error, 1)
	go func() {
		sc := bufio.NewScanner(stdoutR)
		sc.Buffer(make([]byte, 0, 1<<20), 1<<20)
		for sc.Scan() {
			var frame json.RawMessage
			if err := json.Unmarshal(sc.Bytes(), &frame); err != nil {
				scanErr <- fmt.Errorf("non-JSON-RPC bytes on stdout: %q", sc.Bytes())
				return
			}
			frames <- frame
		}
		scanErr <- sc.Err()
	}()

	awaitResponse := func(id int) json.RawMessage {
		t.Helper()
		timeout := time.After(30 * time.Second)
		for {
			select {
			case frame := <-frames:
				var head struct {
					ID json.RawMessage `json:"id"`
				}
				if err := json.Unmarshal(frame, &head); err == nil && string(head.ID) == fmt.Sprint(id) {
					return frame
				}
			case err := <-scanErr:
				t.Fatalf("stdout scan failed (stderr so far: %s): %v", stderr.String(), err)
			case <-timeout:
				t.Fatalf("timeout waiting for response id=%d (stderr so far: %s)", id, stderr.String())
			}
		}
	}

	fmt.Fprintf(stdinW, `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"coverage-driver","version":"0"}}}`+"\n")
	frame := awaitResponse(1)
	if !strings.Contains(string(frame), `"result"`) {
		t.Fatalf("initialize did not return a result frame: %s", frame)
	}

	fmt.Fprintf(stdinW, `{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}`+"\n")
	frame = awaitResponse(2)
	var listResult struct {
		Result struct {
			Tools []struct {
				Name string `json:"name"`
			} `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal(frame, &listResult); err != nil {
		t.Fatalf("parse tools/list response: %v", err)
	}
	names := map[string]bool{}
	for _, tool := range listResult.Result.Tools {
		names[tool.Name] = true
	}
	if !names["vault_get_entry"] || !names["memory_search"] {
		t.Fatalf("expected namespaced tools from both children, got %v", names)
	}

	// Close stdin: the gateway sees EOF and exits cleanly.
	stdinW.Close()
	var code exitcodes.ExitCode
	select {
	case code = <-codeCh:
		if code != exitcodes.ExitOK {
			t.Fatalf("cmdServe = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
		}
	case <-time.After(15 * time.Second):
		t.Fatalf("cmdServe did not exit after stdin close (stderr: %s)", stderr.String())
	}
	// The gateway is done writing; closing the write end lets the
	// scanner see EOF so we can verify the full stdout stream.
	stdoutW.Close()
	if err := <-scanErr; err != nil {
		t.Fatalf("stdout scan failed: %v", err)
	}
}

// ---- buildServers ----

func TestBuildServers_AllDisabledReturnsEmpty(t *testing.T) {
	p := &profile.Profile{
		Name:    "empty",
		Servers: profile.Servers{}, // all disabled by default
	}
	cfg := &config.Config{Servers: config.ServersConfig{}}

	var stderr bytes.Buffer
	servers := buildServers(p, cfg, &stderr, "")

	if len(servers) != 0 {
		t.Fatalf("buildServers with all servers disabled = %d servers, want 0", len(servers))
	}
	if stderr.Len() != 0 {
		t.Errorf("stderr = %q, want empty", stderr.String())
	}
}

func TestBuildServers_MissingBinaryWarnsAndSkips(t *testing.T) {
	p := &profile.Profile{
		Name: "vault-only",
		Servers: profile.Servers{
			Vault: profile.ServerConfig{Enabled: true, Mode: "full"},
		},
	}
	cfg := &config.Config{Servers: config.ServersConfig{
		Vault: config.ServerOverride{BinaryPath: filepath.Join(t.TempDir(), "does-not-exist")},
	}}

	var stderr bytes.Buffer
	servers := buildServers(p, cfg, &stderr, "")

	if len(servers) != 0 {
		t.Fatalf("buildServers with missing binary = %d servers, want 0", len(servers))
	}
	if !strings.Contains(stderr.String(), "vault") {
		t.Errorf("stderr = %q, want a warning naming vault", stderr.String())
	}
}

func TestBuildServers_VaultAgentAddsStdioArgs(t *testing.T) {
	home := sandboxHome(t)
	fakeMCP := buildFakemcpOnce(t)
	wrapper := writeFakeWrapper(t, home, "fakevault",
		`[{"name":"get_entry","description":"fetch"}]`, fakeMCP)

	p := &profile.Profile{
		Name: "agent-test",
		Servers: profile.Servers{
			Vault: profile.ServerConfig{Enabled: true, Mode: "full"},
		},
	}
	cfg := &config.Config{Servers: config.ServersConfig{
		Vault: config.ServerOverride{BinaryPath: wrapper},
	}}

	var stderr bytes.Buffer
	servers := buildServers(p, cfg, &stderr, "my-agent")

	ms, ok := servers["vault"]
	if !ok {
		t.Fatalf("expected a vault ManagedServer, got %v", servers)
	}
	args := ms.Args()
	want := []string{"serve", "--stdio", "--agent", "my-agent", "--allow-locked"}
	if strings.Join(args, " ") != strings.Join(want, " ") {
		t.Errorf("vault args = %v, want %v", args, want)
	}
}

// ---- sync ----

// TestCmdSync_DryRun_NoSymskills covers cmdSync end-to-end in-process:
// a project dir with instructions, dry-run (no writes), and a PATH that
// has no symskills so the skills orchestration degrades to skipped.
func TestCmdSync_DryRun_NoSymskills(t *testing.T) {
	sandboxHome(t)
	project := t.TempDir()
	if err := os.WriteFile(filepath.Join(project, "AGENTS.md"), []byte("# test project\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	// Strip symskills from PATH: the test needs no other external binaries.
	t.Setenv("PATH", t.TempDir())

	var stdout, stderr bytes.Buffer
	code := cmdSync([]string{"--dry-run", "--project", project, "claude"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdSync = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	out := stdout.String()
	if !strings.Contains(out, "claude:") {
		t.Errorf("stdout = %q, want a claude target row", out)
	}
	if !strings.Contains(out, "symskills not found") {
		t.Errorf("stdout = %q, want the symskills-absent hint", out)
	}
}

func TestCmdSync_UnknownCommandFlag(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdSync([]string{"--output", "bogus"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdSync with bogus output format = %d, want %d", code, exitcodes.ExitNoInput)
	}
}

// ---- profile wrappers (direct entry points) ----

func TestCmdProfileList_DirectEntry(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "alpha", "[profile]\nname = \"alpha\"\ndescription = \"first\"\n")
	writeProfileFile(t, home, "beta", "[profile]\nname = \"beta\"\n")

	var stdout, stderr bytes.Buffer
	code := cmdProfileList([]string{"--json"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdProfileList = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	out := stdout.String()
	if !strings.Contains(out, `"name":"alpha"`) || !strings.Contains(out, `"name":"beta"`) {
		t.Errorf("stdout = %q, want both profile names as JSON", out)
	}
}

func TestCmdProfileShow_DirectEntry(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "warny", "[profile]\nname = \"warny\"\ndescription = \"desc\"\n")

	var stdout, stderr bytes.Buffer
	code := cmdProfileShow([]string{"--json", "warny"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdProfileShow = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	out := stdout.String()
	if !strings.Contains(out, `"name":"warny"`) || !strings.Contains(out, `"description":"desc"`) {
		t.Errorf("stdout = %q, want profile name and description as JSON", out)
	}
}

// ---- audit ----

func TestCmdAuditTail_ProfileFilter(t *testing.T) {
	home := sandboxHome(t)
	auditDir := filepath.Join(home, ".local", "share", "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	for i, tool := range []string{"first", "second", "third"} {
		entry := map[string]any{
			"timestamp":   fmt.Sprintf("2026-01-01T00:00:%02dZ", i),
			"profile":     "personal",
			"server":      "memory",
			"tool":        tool,
			"duration_ms": i,
			"status":      "ok",
		}
		data, _ := json.Marshal(entry)
		buf.Write(data)
		buf.WriteByte('\n')
	}
	if err := os.WriteFile(filepath.Join(auditDir, "personal.jsonl"), buf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdAudit([]string{"tail", "--profile", "personal", "-n", "2"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdAudit tail = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	out := stdout.String()
	if !strings.Contains(out, "second") || !strings.Contains(out, "third") {
		t.Errorf("stdout = %q, want the two newest entries", out)
	}
	if strings.Contains(out, "first") {
		t.Errorf("stdout = %q, oldest entry should be cut by -n 2", out)
	}
}

func TestCmdAudit_UnknownSubcommand(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdAudit([]string{"bogus"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("audit bogus = %d, want %d", code, exitcodes.ExitNoInput)
	}
}
