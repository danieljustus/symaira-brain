package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// TestStdioHygiene_ServeEmitsOnlyJSONRPC is the CI hygiene gate for the
// MCP gateway: across startup, initialize, tools/list, a failing
// tools/call and shutdown, `symbrain serve` must emit nothing but
// JSON-RPC frames on stdout. Diagnostics belong on stderr; any other
// stdout byte breaks the harness transport.
func TestStdioHygiene_ServeEmitsOnlyJSONRPC(t *testing.T) {
	symbrainBin := buildSymbrainOnce(t)
	fakeMCP := buildFakemcpOnce(t)

	home := t.TempDir()

	// Two fake children with distinct tool sets, one per state core used
	// by the profile. Wrapper scripts pin FAKEMCP_TOOLS per child because
	// children inherit symbrain's environment.
	vaultWrapper := writeFakeWrapper(t, home, "fakevault",
		`[{"name":"get_entry","description":"fetch"},{"name":"health","description":"hc"}]`, fakeMCP)
	memoryWrapper := writeFakeWrapper(t, home, "fakememory",
		`[{"name":"memory_search","description":"search"},{"name":"memory_set","description":"store"},{"name":"boom","description":"fails","behavior":"toolerror"}]`, fakeMCP)

	profileDir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(profileDir, 0o755); err != nil {
		t.Fatal(err)
	}
	profileTOML := `[profile]
name = "hygiene"

[servers.vault]
enabled = true
mode = "full"

[servers.memory]
enabled = true
mode = "read_write"
tools_deny = ["memory_set"]

[servers.skills]
enabled = false

[audit]
enabled = false
`
	if err := os.WriteFile(filepath.Join(profileDir, "hygiene.toml"), []byte(profileTOML), 0o600); err != nil {
		t.Fatal(err)
	}

	cmd := exec.Command(symbrainBin, "serve", "--profile", "hygiene")
	cmd.Env = append(os.Environ(),
		"HOME="+home,
		"SYMBRAIN_SERVERS_VAULT_BINARY_PATH="+vaultWrapper,
		"SYMBRAIN_SERVERS_MEMORY_BINARY_PATH="+memoryWrapper,
	)

	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var stderr strings.Builder
	cmd.Stderr = &stderr

	if err := cmd.Start(); err != nil {
		t.Fatalf("start symbrain serve: %v", err)
	}

	frames := make(chan json.RawMessage)
	scanErr := make(chan error, 1)
	go func() {
		sc := bufio.NewScanner(stdout)
		sc.Buffer(make([]byte, 0, 1<<20), 1<<20)
		for sc.Scan() {
			line := sc.Bytes()
			if len(strings.TrimSpace(string(line))) == 0 {
				scanErr <- fmt.Errorf("blank line on stdout")
				return
			}
			var frame json.RawMessage
			if err := json.Unmarshal(line, &frame); err != nil {
				scanErr <- fmt.Errorf("non-JSON-RPC bytes on stdout: %q", line)
				return
			}
			var head struct {
				JSONRPC string          `json:"jsonrpc"`
				ID      json.RawMessage `json:"id"`
			}
			if err := json.Unmarshal(line, &head); err != nil || head.JSONRPC != "2.0" {
				scanErr <- fmt.Errorf("stdout line is not a JSON-RPC 2.0 frame: %q", line)
				return
			}
			frames <- frame
		}
		scanErr <- sc.Err()
	}()

	send := func(id int, method string, params string) {
		t.Helper()
		fmt.Fprintf(stdin, `{"jsonrpc":"2.0","id":%d,"method":%q,"params":%s}`+"\n", id, method, params)
	}
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

	// Startup + initialize.
	send(1, "initialize", `{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"hygiene-gate","version":"0"}}`)
	awaitResponse(1)

	// One tools/list.
	send(2, "tools/list", `{}`)
	listFrame := awaitResponse(2)
	var listResult struct {
		Result struct {
			Tools []struct {
				Name string `json:"name"`
			} `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal(listFrame, &listResult); err != nil {
		t.Fatalf("parse tools/list response: %v", err)
	}
	if len(listResult.Result.Tools) == 0 {
		t.Fatal("tools/list returned zero tools")
	}
	names := map[string]bool{}
	for _, tool := range listResult.Result.Tools {
		names[tool.Name] = true
	}
	if !names["vault_get_entry"] || !names["memory_search"] {
		t.Fatalf("expected namespaced vault_ and passthrough memory_ tools, got %v", names)
	}
	if names["memory_set"] {
		t.Fatalf("tools_deny failed: memory_set present in tools/list: %v", names)
	}

	// A failing tools/call: the child answers with isError:true.
	send(3, "tools/call", `{"name":"boom","arguments":{}}`)
	awaitResponse(3)

	// A policy-hidden tool is absent from tools/list and calling it
	// yields a JSON-RPC error frame (never plain text on stdout).
	send(4, "tools/call", `{"name":"memory_set","arguments":{}}`)
	hiddenFrame := awaitResponse(4)
	if !strings.Contains(string(hiddenFrame), `"error"`) {
		t.Fatalf("policy-hidden memory_set did not return an error frame: %s", hiddenFrame)
	}

	// Shutdown: close stdin, expect a clean exit and a scanner that saw
	// only valid frames to the very end.
	if err := stdin.Close(); err != nil {
		t.Fatal(err)
	}
	waitErr := make(chan error, 1)
	go func() { waitErr <- cmd.Wait() }()
	select {
	case err := <-waitErr:
		if err != nil {
			t.Fatalf("symbrain serve exited with error: %v (stderr: %s)", err, stderr.String())
		}
	case <-time.After(15 * time.Second):
		_ = cmd.Process.Kill()
		t.Fatalf("symbrain serve did not exit after stdin close (stderr: %s)", stderr.String())
	}
	if err := <-scanErr; err != nil {
		t.Fatalf("stdout hygiene violated: %v", err)
	}
}

var (
	symbrainOnce     sync.Once
	symbrainBinPath  string
	symbrainBuildErr error
)

func buildSymbrainOnce(t *testing.T) string {
	t.Helper()
	symbrainOnce.Do(func() {
		dir, err := os.MkdirTemp("", "symbrain-hygiene-*")
		if err != nil {
			symbrainBuildErr = err
			return
		}
		symbrainBinPath = filepath.Join(dir, "symbrain")
		cmd := exec.Command("go", "build", "-o", symbrainBinPath, ".")
		cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
		if out, err := cmd.CombinedOutput(); err != nil {
			symbrainBuildErr = fmt.Errorf("go build symbrain: %v\n%s", err, out)
		}
	})
	if symbrainBuildErr != nil {
		t.Fatal(symbrainBuildErr)
	}
	return symbrainBinPath
}

var (
	fakemcpOnce     sync.Once
	fakemcpPath     string
	fakemcpBuildErr error
)

func buildFakemcpOnce(t *testing.T) string {
	t.Helper()
	fakemcpOnce.Do(func() {
		dir, err := os.MkdirTemp("", "symbrain-hygiene-fake-*")
		if err != nil {
			fakemcpBuildErr = err
			return
		}
		fakemcpPath = filepath.Join(dir, "fakemcp")
		cmd := exec.Command("go", "build", "-o", fakemcpPath, "../../internal/broker/testdata/fakemcp")
		cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
		if out, err := cmd.CombinedOutput(); err != nil {
			fakemcpBuildErr = fmt.Errorf("go build fakemcp: %v\n%s", err, out)
		}
	})
	if fakemcpBuildErr != nil {
		t.Fatal(fakemcpBuildErr)
	}
	return fakemcpPath
}

// writeFakeWrapper creates an executable shell wrapper that pins
// FAKEMCP_TOOLS for one fake child before exec'ing the shared fakemcp
// binary, so each state core in a test exposes a distinct tool set.
func writeFakeWrapper(t *testing.T, dir, name, toolsJSON, fakeBin string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	content := "#!/bin/sh\nFAKEMCP_TOOLS='" + toolsJSON + "' exec " + fakeBin + "\n"
	if err := os.WriteFile(path, []byte(content), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}
