package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func browseFakeWrapper(t *testing.T, dir, fakeBin, marker string) string {
	t.Helper()
	path := filepath.Join(dir, "symbrowse-fake")
	content := "#!/bin/sh\nprintf '%s\\n' \"$@\" > " + marker + "\n" +
		"FAKEMCP_TOOLS='[{\"name\":\"search\",\"description\":\"safe search\",\"annotations\":{\"readOnlyHint\":true}},{\"name\":\"write\",\"description\":\"write operation\",\"annotations\":{\"readOnlyHint\":false}}]' exec " + fakeBin + "\n"
	if err := os.WriteFile(path, []byte(content), 0o755); err != nil {
		t.Fatal(err)
	}
	return path
}

func browseProfileFile(t *testing.T, dir, command string, enabled bool) string {
	t.Helper()
	path := filepath.Join(dir, "browse.toml")
	fmt.Fprintf(mustCreate(t, path), "[profile]\nname = \"browse-acceptance\"\n\n[servers.browse]\nenabled = %t\ncommand = %q\nargs = [\"mcp\"]\naccess = \"read\"\n\n[audit]\nenabled = false\n", enabled, command)
	return path
}

func mustCreate(t *testing.T, path string) *os.File {
	t.Helper()
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = f.Close() })
	return f
}

func TestBrowseBrainProfileRoutesMCPWithPolicyAndExactArgs(t *testing.T) {
	home := sandboxHome(t)
	fakeMCP := buildFakemcpOnce(t)
	marker := filepath.Join(t.TempDir(), "argv")
	wrapper := browseFakeWrapper(t, home, fakeMCP, marker)
	profilePath := browseProfileFile(t, t.TempDir(), wrapper, true)

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
	t.Cleanup(func() {
		os.Stdin, os.Stdout = oldStdin, oldStdout
		_ = stdinR.Close()
		_ = stdoutW.Close()
	})

	var diagnostics lockedBuffer
	codeCh := make(chan exitcodes.ExitCode, 1)
	go func() {
		codeCh <- cmdMcp([]string{"--profile-file", profilePath}, &diagnostics, &diagnostics)
	}()

	frames := make(chan json.RawMessage, 8)
	scanErr := make(chan error, 1)
	go func() {
		scanner := bufio.NewScanner(stdoutR)
		for scanner.Scan() {
			var frame json.RawMessage
			if err := json.Unmarshal(scanner.Bytes(), &frame); err != nil {
				scanErr <- err
				return
			}
			frames <- frame
		}
		scanErr <- scanner.Err()
	}()
	await := func(id int) json.RawMessage {
		t.Helper()
		deadline := time.After(20 * time.Second)
		for {
			select {
			case frame := <-frames:
				var header struct {
					ID json.RawMessage `json:"id"`
				}
				if json.Unmarshal(frame, &header) == nil && string(header.ID) == fmt.Sprint(id) {
					return frame
				}
			case err := <-scanErr:
				t.Fatalf("Browse gateway stdout scan: %v (stderr: %s)", err, diagnostics.String())
			case <-deadline:
				t.Fatalf("timeout waiting for MCP response %d (stderr: %s)", id, diagnostics.String())
			}
		}
	}
	send := func(id int, method, params string) {
		_, _ = fmt.Fprintf(stdinW, `{"jsonrpc":"2.0","id":%d,"method":%q,"params":%s}`+"\n", id, method, params)
	}

	send(1, "initialize", `{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"browse-acceptance","version":"0"}}`)
	await(1)
	send(2, "tools/list", `{}`)
	listFrame := await(2)
	var list struct {
		Result struct {
			Tools []struct {
				Name string `json:"name"`
			} `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal(listFrame, &list); err != nil {
		t.Fatal(err)
	}
	names := map[string]bool{}
	for _, tool := range list.Result.Tools {
		names[tool.Name] = true
	}
	if !names["search"] || names["write"] {
		t.Fatalf("Browse policy/catalog = %v, want search only", names)
	}
	send(3, "tools/call", `{"name":"search","arguments":{"query":"safe-fixture"}}`)
	callFrame := await(3)
	if strings.Contains(string(callFrame), `"error"`) || !strings.Contains(string(callFrame), "safe-fixture") {
		t.Fatalf("Browse tools/call frame = %s, want forwarded echo", callFrame)
	}
	if err := stdinW.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-codeCh:
		if code != exitcodes.ExitOK {
			t.Fatalf("cmdMcp = %d (stderr: %s)", code, diagnostics.String())
		}
	case <-time.After(10 * time.Second):
		t.Fatal("cmdMcp did not shut down after stdin close")
	}
	stdoutW.Close()
	if err := <-scanErr; err != nil {
		t.Fatal(err)
	}
	argv, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("read Browse argv marker: %v", err)
	}
	if got := strings.TrimSpace(string(argv)); got != "mcp" {
		t.Fatalf("Browse child argv = %q, want exactly %q", got, "mcp")
	}
}

func TestBrowseDisabledProfileDoesNotSpawn(t *testing.T) {
	home := sandboxHome(t)
	fakeMCP := buildFakemcpOnce(t)
	marker := filepath.Join(t.TempDir(), "spawn-marker")
	wrapper := browseFakeWrapper(t, home, fakeMCP, marker)
	p := &profile.Profile{Name: "disabled", Servers: profile.Servers{
		"browse": {Enabled: false, Command: wrapper, Args: []string{"mcp"}},
	}}
	var stderr bytes.Buffer
	servers := buildServers(p, &config.Config{}, &stderr, "")
	if _, ok := servers["browse"]; ok {
		t.Fatal("disabled Browse profile was added to managed servers")
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatalf("disabled Browse profile spawned child; marker stat = %v", err)
	}
}

func TestBrowseMissingBinaryDegradesWithoutBlockingAnotherServer(t *testing.T) {
	home := sandboxHome(t)
	fakeMCP := buildFakemcpOnce(t)
	vault := writeFakeWrapper(t, home, "fakevault", `[{"name":"health","description":"healthy"}]`, fakeMCP)
	p := &profile.Profile{Name: "degraded", Servers: profile.Servers{
		"vault":  {Enabled: true, Mode: profile.VaultModeFull},
		"browse": {Enabled: true, Command: "symbrowse-missing-for-acceptance", Args: []string{"mcp"}},
	}}
	cfg := &config.Config{Servers: config.ServersConfig{Vault: config.ServerOverride{BinaryPath: vault}}}
	var stderr bytes.Buffer
	servers := buildServers(p, cfg, &stderr, "")
	if _, ok := servers["browse"]; ok {
		t.Fatal("missing Browse binary should not create a managed server")
	}
	if !strings.Contains(stderr.String(), "browse") {
		t.Fatalf("stderr = %q, want named Browse degradation", stderr.String())
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	tools, err := servers["vault"].ListTools(ctx)
	if err != nil || len(tools) != 1 || tools[0].Name != "health" {
		t.Fatalf("other enabled server after Browse degradation = %v, %v", tools, err)
	}
	servers["vault"].Shutdown()
}

func TestBrowseStandaloneCLIStaysDirect(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	browseDir := filepath.Join(filepath.Dir(file), "..", "..", "browse")
	cmd := exec.Command("go", "run", "./cmd/symbrowse", "version", "--json")
	cmd.Dir = browseDir
	cmd.Env = os.Environ()
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("standalone symbrowse version smoke: %v\n%s", err, out)
	}
	if !strings.Contains(string(out), `"tool":"symbrowse"`) || !strings.Contains(string(out), `"schema_version":8`) {
		t.Fatalf("standalone symbrowse output = %q", out)
	}
}
