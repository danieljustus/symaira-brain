package main

import (
	"bytes"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/google/uuid"
)

// freeTCPPort allocates an ephemeral loopback port and immediately releases
// it, mirroring the "bind, read the port, close" pattern already used by
// internal/memory/mcp's own StartHTTPServer tests (server_test.go). There is
// an inherent, accepted race between releasing the port here and
// StartHTTPServer rebinding it a moment later.
func freeTCPPort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("failed to allocate a free port: %v", err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	if err := ln.Close(); err != nil {
		t.Fatalf("failed to release allocated port: %v", err)
	}
	return port
}

// waitForServeStatus polls /api/status until the server started by
// cmdMemoryServe/buildMemoryHTTPServer is accepting connections, or fails
// the test once the deadline passes.
func waitForServeStatus(t *testing.T, port int) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	url := "http://127.0.0.1:" + strconv.Itoa(port) + "/api/status"
	for time.Now().Before(deadline) {
		resp, err := http.Get(url)
		if err == nil {
			_ = resp.Body.Close()
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("symbrain memory serve did not become ready on port %d in time", port)
}

// TestCmdMemoryServe_SyncRoundTrip proves out issue #426's acceptance
// criterion end to end: `symbrain memory serve` (via buildMemoryHTTPServer +
// StartHTTPServer, exactly the code path cmdMemoryServe runs) accepts a real
// `symbrain memory sync --remote` client and moves memories in both
// directions. This is the local round-trip test the issue asks for, reusing
// the existing syncclient/http_server test patterns (free-port allocation +
// os.Interrupt shutdown from server_test.go; --db-path client wiring from
// cmd_memory_test.go) instead of inventing new test infrastructure.
func TestCmdMemoryServe_SyncRoundTrip(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "memory-serve-roundtrip-secret")

	serverDBPath := filepath.Join(t.TempDir(), "serve-remote.db")
	srv, closeDB, err := buildMemoryHTTPServer(serverDBPath)
	if err != nil {
		t.Fatalf("buildMemoryHTTPServer: %v", err)
	}
	defer closeDB()

	port := freeTCPPort(t)
	done := make(chan error, 1)
	go func() { done <- srv.StartHTTPServer(port) }()
	t.Cleanup(func() {
		proc, err := os.FindProcess(os.Getpid())
		if err == nil {
			_ = proc.Signal(os.Interrupt)
		}
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			t.Error("symbrain memory serve did not shut down after interrupt")
		}
	})
	waitForServeStatus(t, port)

	// Mint a client token against the same JWT secret the server resolved
	// from JWT_SECRET_KEY (see security.NewJWTProvider's resolution order).
	jwtProvider, err := security.NewJWTProvider(config.Defaults(), nil)
	if err != nil {
		t.Fatalf("new JWT provider: %v", err)
	}
	token, err := jwtProvider.GenerateToken("memory-serve-roundtrip", time.Hour)
	if err != nil {
		t.Fatalf("generate token: %v", err)
	}

	// A second, independent local database plays the sync client.
	localCfg := config.Defaults()
	localCfg.Database.Path = filepath.Join(t.TempDir(), "serve-local.db")
	localDB, err := db.Open(localCfg)
	if err != nil {
		t.Fatalf("open local database: %v", err)
	}

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	if _, err := srv.DB().UpsertMemoryIfNewer(&db.Memory{
		ID: remoteID, Content: "fact living on the memory-serve peer", Scope: "global",
		CreatedAt: base.Add(2 * time.Second), UpdatedAt: base.Add(2 * time.Second),
	}); err != nil {
		t.Fatalf("seed remote memory: %v", err)
	}
	if _, err := localDB.UpsertMemoryIfNewer(&db.Memory{
		ID: localID, Content: "fact living on the sync client", Scope: "global",
		CreatedAt: base.Add(time.Second), UpdatedAt: base.Add(time.Second),
	}); err != nil {
		t.Fatalf("seed local memory: %v", err)
	}
	if err := localDB.Close(); err != nil {
		t.Fatalf("close local db before CLI sync: %v", err)
	}

	remoteURL := "http://127.0.0.1:" + strconv.Itoa(port)
	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{
		"--remote", remoteURL,
		"--token", token,
		"--db", localCfg.Database.Path,
	}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("memory sync --remote against memory serve failed: exit=%v stdout=%q stderr=%q", code, stdout.String(), stderr.String())
	}

	reopened, err := db.Open(&config.Config{Database: config.DatabaseConfig{Path: localCfg.Database.Path}})
	if err != nil {
		t.Fatalf("reopen local db: %v", err)
	}
	defer func() { _ = reopened.Close() }()

	pulled, err := reopened.GetMemory(remoteID)
	if err != nil || pulled == nil || pulled.Content != "fact living on the memory-serve peer" {
		t.Errorf("local db missing memory pulled from memory serve: %+v err=%v", pulled, err)
	}
	pushed, err := srv.DB().GetMemory(localID)
	if err != nil || pushed == nil || pushed.Content != "fact living on the sync client" {
		t.Errorf("memory serve peer missing memory pushed by the client: %+v err=%v", pushed, err)
	}
}
