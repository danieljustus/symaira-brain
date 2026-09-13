package broker

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func waitForRestart(t *testing.T, ms *ManagedServer) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for ms.RestartCount() != 1 {
		if time.Now().After(deadline) {
			t.Fatal("restart was not scheduled")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func TestManagedServer_CancelledRequestDuringBackoffDoesNotSpawn(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawns.log")
	ms := NewManagedServer(ServerConfig{Name: "cancel", BinaryPath: fakeBinPath, MaxRestarts: 1, BackoffBase: 500 * time.Millisecond, Env: append(os.Environ(), "FAKEMCP_SPAWN_MARKER="+marker), Logger: testLogger(t)})
	defer ms.Shutdown()
	if _, err := ms.ListTools(context.Background()); err != nil {
		t.Fatal(err)
	}
	_, _ = ms.CallTool(context.Background(), "crash", nil)
	waitForRestart(t, ms)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := ms.ListTools(ctx)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled request error = %v", err)
	}
	time.Sleep(750 * time.Millisecond)
	assertSpawnCount(t, marker, 2)
	if got := ms.State(); got != StateReady {
		t.Fatalf("state after scheduled restart = %v, want ready", got)
	}
}

func TestManagedServer_ShutdownDuringBackoffDoesNotSpawn(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawns.log")
	ms := NewManagedServer(ServerConfig{Name: "shutdown", BinaryPath: fakeBinPath, MaxRestarts: 1, BackoffBase: 500 * time.Millisecond, Env: append(os.Environ(), "FAKEMCP_SPAWN_MARKER="+marker), Logger: testLogger(t)})
	if _, err := ms.ListTools(context.Background()); err != nil {
		t.Fatal(err)
	}
	_, _ = ms.CallTool(context.Background(), "crash", nil)
	waitForRestart(t, ms)
	ms.Shutdown()
	time.Sleep(750 * time.Millisecond)
	assertSpawnCount(t, marker, 1)
	if got := ms.State(); got != StateStopped {
		t.Fatalf("state = %v, want stopped", got)
	}
}

func TestManagedServer_LifecycleDiagnosticsDoNotExposeArguments(t *testing.T) {
	var logs bytes.Buffer
	secret := "sensitive-process-argument-value"
	logger := slog.New(slog.NewTextHandler(&logs, nil))
	ms := NewManagedServer(ServerConfig{Name: "diagnostic", BinaryPath: fakeBinPath, Args: []string{"--token", secret}, MaxRestarts: 0, Logger: logger})
	defer ms.Shutdown()
	if _, err := ms.ListTools(context.Background()); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(logs.String(), secret) {
		t.Fatalf("lifecycle logs exposed raw argument: %q", logs.String())
	}
}
