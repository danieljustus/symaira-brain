package broker

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"
)

// testLogger routes broker log output to t.Log for diagnosability.
func testLogger(t *testing.T) *slog.Logger {
	return slog.New(slog.NewTextHandler(testLogWriter{t}, &slog.HandlerOptions{Level: slog.LevelDebug}))
}

type testLogWriter struct{ t *testing.T }

func (w testLogWriter) Write(p []byte) (int, error) {
	w.t.Logf("broker: %s", p)
	return len(p), nil
}

func TestManagedServer_StateTransitions(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 0,
		Logger:      testLogger(t),
	})

	if got := ms.State(); got != StateIdle {
		t.Fatalf("initial state = %v, want StateIdle", got)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	_, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}

	if got := ms.State(); got != StateReady {
		t.Fatalf("after ListTools state = %v, want StateReady", got)
	}

	ms.Shutdown()
	if got := ms.State(); got != StateStopped {
		t.Fatalf("after Shutdown state = %v, want StateStopped", got)
	}
}

func TestManagedServer_LazySpawn(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 0,
		Logger:      testLogger(t),
	})
	defer ms.Shutdown()

	// Before any call, no process should exist.
	if ms.RestartCount() != 0 {
		t.Fatalf("RestartCount = %d before any call, want 0", ms.RestartCount())
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	tools, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}
	if len(tools) == 0 {
		t.Fatal("ListTools() returned empty tool list")
	}

	// Verify the child is spawned by checking Pid.
	c := ms.client.Load()
	if c == nil || c.Pid() == 0 {
		t.Fatal("child not spawned after ListTools()")
	}
}

func TestManagedServer_CrashRestart_Degraded(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 0, // No restarts — go straight to degraded on crash.
		Logger:      testLogger(t),
	})
	defer ms.Shutdown()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// Spawn the child.
	if _, err := ms.ListTools(ctx); err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}

	// Trigger a crash.
	_, err := ms.CallTool(ctx, "crash", nil)
	if err == nil {
		t.Fatal("CallTool(crash) returned nil error, want error")
	}

	// Wait for the child to exit and the watchChild goroutine to run.
	time.Sleep(200 * time.Millisecond)

	if got := ms.State(); got != StateDegraded {
		t.Fatalf("after crash with MaxRestarts=0: state = %v, want StateDegraded", got)
	}

	// Subsequent calls should fail immediately.
	_, err = ms.ListTools(ctx)
	if err == nil {
		t.Fatal("ListTools() after degraded returned nil error")
	}
	var closedErr *ClosedError
	if !errors.As(err, &closedErr) {
		t.Fatalf("error = %v (%T), want *ClosedError", err, err)
	}
}

func TestManagedServer_CrashRestart_RestartsOnce(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 1,
		BackoffBase: 10 * time.Millisecond, // Fast for tests.
		Logger:      testLogger(t),
	})
	defer ms.Shutdown()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Spawn.
	if _, err := ms.ListTools(ctx); err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}

	// Crash.
	_, _ = ms.CallTool(ctx, "crash", nil)

	// Wait for restart to complete.
	time.Sleep(500 * time.Millisecond)

	if got := ms.State(); got != StateReady {
		t.Fatalf("after 1 crash with MaxRestarts=1: state = %v, want StateReady", got)
	}

	// The server should be usable again.
	tools, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools() after restart error = %v", err)
	}
	if len(tools) == 0 {
		t.Fatal("ListTools() after restart returned empty tool list")
	}
}

func TestManagedServer_CrashRestart_ConcurrentRequest_SingleChild(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawns.log")
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 1,
		BackoffBase: 300 * time.Millisecond, // Long enough for a concurrent request to win the race.
		Env:         append(os.Environ(), "FAKEMCP_SPAWN_MARKER="+marker),
		Logger:      testLogger(t),
	})
	defer ms.Shutdown()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Initial spawn.
	if _, err := ms.ListTools(ctx); err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}
	assertSpawnCount(t, marker, 1)

	// Crash the child.
	_, _ = ms.CallTool(ctx, "crash", nil)

	// Wait until watchChild has cleared the client and scheduled the
	// delayed restart. RestartCount is guarded by ms.mu, so observing 1
	// means the schedule happened and the lock was released.
	deadline := time.Now().Add(5 * time.Second)
	for ms.RestartCount() != 1 {
		if time.Now().After(deadline) {
			t.Fatal("crash restart was not scheduled within 5s")
		}
		time.Sleep(10 * time.Millisecond)
	}

	// A request arriving during the backoff window spawns the replacement.
	if _, err := ms.ListTools(ctx); err != nil {
		t.Fatalf("ListTools() during backoff error = %v", err)
	}
	if got := ms.State(); got != StateReady {
		t.Fatalf("state after concurrent restart = %v, want StateReady", got)
	}

	// Let the scheduled restart fire; it must not spawn a duplicate child.
	time.Sleep(600 * time.Millisecond)

	assertSpawnCount(t, marker, 2)

	// The server remains usable.
	tools, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools() after race error = %v", err)
	}
	if len(tools) == 0 {
		t.Fatal("ListTools() after race returned empty tool list")
	}
}

// assertSpawnCount fails the test unless the fakemcp spawn marker file
// records exactly want spawned child processes.
func assertSpawnCount(t *testing.T, marker string, want int) {
	t.Helper()
	data, err := os.ReadFile(marker)
	if err != nil {
		t.Fatalf("read spawn marker: %v", err)
	}
	if got := strings.Count(string(data), "\n"); got != want {
		t.Fatalf("spawned child processes = %d, want %d (marker: %q)", got, want, data)
	}
}

func TestManagedServer_ShutdownIdempotent(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:       "test",
		BinaryPath: fakeBinPath,
		Logger:     testLogger(t),
	})

	ms.Shutdown()
	ms.Shutdown() // Should not panic.

	if got := ms.State(); got != StateStopped {
		t.Fatalf("state = %v, want StateStopped", got)
	}
}

func TestManagedServer_NoZombieProcesses(t *testing.T) {
	ms := NewManagedServer(ServerConfig{
		Name:        "test",
		BinaryPath:  fakeBinPath,
		MaxRestarts: 0,
		Logger:      testLogger(t),
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if _, err := ms.ListTools(ctx); err != nil {
		t.Fatalf("ListTools() error = %v", err)
	}

	c := ms.client.Load()
	pid := c.Pid()
	if pid == 0 {
		t.Fatal("child Pid() = 0")
	}

	ms.Shutdown()

	// Wait for process to be reaped.
	select {
	case <-c.Exited():
	case <-time.After(5 * time.Second):
		t.Fatal("child process was not reaped after Shutdown()")
	}

	// Verify process is dead (kill with signal 0).
	p, err := os.FindProcess(pid)
	if err != nil {
		t.Fatalf("FindProcess(%d): %v", pid, err)
	}
	err = p.Signal(syscall.Signal(0))
	// On Darwin, signal 0 to a dead process returns ESRCH.
	if err == nil {
		t.Errorf("process %d still alive after Shutdown()", pid)
	}
}
