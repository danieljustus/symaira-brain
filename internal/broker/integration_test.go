package broker

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

// fakeBinPath is the path to the fakemcp fixture binary (see #12), built
// once in TestMain and reused by every integration test in this file.
var fakeBinPath string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "symbrain-fakemcp-*")
	if err != nil {
		panic("broker: fakemcp build: mkdtemp: " + err.Error())
	}
	defer os.RemoveAll(dir)

	fakeBinPath = filepath.Join(dir, "fakemcp")
	if runtimeIsWindows() {
		fakeBinPath += ".exe"
	}

	cmd := exec.Command("go", "build", "-o", fakeBinPath, "./testdata/fakemcp")
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if out, err := cmd.CombinedOutput(); err != nil {
		panic("broker: failed to build fakemcp fixture: " + err.Error() + "\n" + string(out))
	}

	os.Exit(m.Run())
}

func runtimeIsWindows() bool {
	return os.PathSeparator == '\\'
}

func spawnFake(t *testing.T, env map[string]string) *Client {
	t.Helper()
	opts := Options{Stderr: testStderr{t}}
	if len(env) > 0 {
		full := os.Environ()
		for k, v := range env {
			full = append(full, k+"="+v)
		}
		opts.Env = full
	}
	c, err := Spawn(fakeBinPath, opts)
	if err != nil {
		t.Fatalf("Spawn(fakemcp): %v", err)
	}
	t.Cleanup(func() {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	})
	return c
}

// testStderr routes a spawned fakemcp's stderr into t.Log instead of the
// test process's real stderr, so a failing fixture is easy to diagnose
// without polluting `go test` output on the happy path (t.Log is only
// shown for failing/verbose tests).
type testStderr struct{ t *testing.T }

func (w testStderr) Write(p []byte) (int, error) {
	w.t.Logf("fakemcp stderr: %s", p)
	return len(p), nil
}

func TestIntegration_VersionJSON(t *testing.T) {
	out, err := exec.Command(fakeBinPath, "version", "--json").Output()
	if err != nil {
		t.Fatalf("fakemcp version --json: %v", err)
	}
	var payload struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(out, &payload); err != nil {
		t.Fatalf("parse version output %q: %v", out, err)
	}
	if payload.Version == "" {
		t.Error("version is empty")
	}
}

func TestIntegration_Handshake(t *testing.T) {
	c := spawnFake(t, nil)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	result, err := c.Initialize(ctx)
	if err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}
	if result.ServerInfo.Name != "fakemcp" {
		t.Errorf("ServerInfo.Name = %q, want fakemcp", result.ServerInfo.Name)
	}
	if result.ProtocolVersion == "" {
		t.Error("ProtocolVersion is empty")
	}
}

func TestIntegration_ListTools(t *testing.T) {
	c := spawnFake(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	tools, err := c.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}

	names := make(map[string]bool, len(tools))
	for _, tool := range tools {
		names[tool.Name] = true
	}
	for _, want := range []string{"echo", "crash", "slow", "toolerror"} {
		if !names[want] {
			t.Errorf("tools/list missing default tool %q (got %v)", want, names)
		}
	}
}

func TestIntegration_CallTool_Echo(t *testing.T) {
	c := spawnFake(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	result, err := c.CallTool(ctx, "echo", json.RawMessage(`{"greeting":"hi"}`))
	if err != nil {
		t.Fatalf("CallTool(echo) error = %v", err)
	}
	if result.IsError {
		t.Fatalf("IsError = true, content = %+v", result.Content)
	}
	if len(result.Content) != 1 || result.Content[0].Text != `{"greeting":"hi"}` {
		t.Fatalf("Content = %+v", result.Content)
	}
}

func TestIntegration_CallTool_ToolError(t *testing.T) {
	c := spawnFake(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	result, err := c.CallTool(ctx, "toolerror", nil)
	if err != nil {
		t.Fatalf("CallTool(toolerror) error = %v, want nil (tool errors are not Go errors)", err)
	}
	if !result.IsError {
		t.Fatalf("IsError = false, want true")
	}
}

func TestIntegration_CallTool_UnknownTool_RPCError(t *testing.T) {
	c := spawnFake(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	_, err := c.CallTool(ctx, "does_not_exist", nil)
	var rpcErr *RPCError
	if !errors.As(err, &rpcErr) {
		t.Fatalf("error = %v (%T), want *RPCError", err, err)
	}
}

func TestIntegration_CallTool_Timeout(t *testing.T) {
	c := spawnFake(t, map[string]string{"FAKEMCP_SLOW_MS": "3000"})
	initCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if _, err := c.Initialize(initCtx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	callCtx, cancelCall := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancelCall()

	_, err := c.CallTool(callCtx, "slow", nil)
	var timeoutErr *TimeoutError
	if !errors.As(err, &timeoutErr) {
		t.Fatalf("error = %v (%T), want *TimeoutError", err, err)
	}
}

func TestIntegration_Crash(t *testing.T) {
	c := spawnFake(t, nil)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if _, err := c.Initialize(ctx); err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}

	_, err := c.CallTool(ctx, "crash", nil)
	var closedErr *ClosedError
	if !errors.As(err, &closedErr) {
		t.Fatalf("error = %v (%T), want *ClosedError", err, err)
	}

	select {
	case <-c.Exited():
	case <-time.After(5 * time.Second):
		t.Fatal("child process was not reaped after crashing")
	}
	if c.ExitErr() == nil {
		t.Error("ExitErr() = nil, want a non-zero exit error after os.Exit(1)")
	}

	// A call after the crash must fail immediately, not hang.
	_, err = c.CallTool(ctx, "echo", nil)
	if !errors.As(err, &closedErr) {
		t.Fatalf("post-crash call error = %v (%T), want *ClosedError", err, err)
	}
}
