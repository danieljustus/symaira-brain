package main

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/syncclient"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/google/uuid"
)

// cliTestServer is an in-process memory server speaking the real HTTP sync
// API on a random localhost port — the "remote" for CLI tests. No external
// binary is involved.
type cliTestServer struct {
	db    *db.DB
	token string
	url   string
}

func newCLITestServer(t *testing.T) *cliTestServer {
	t.Helper()
	t.Setenv("JWT_SECRET_KEY", "cmd-test-shared-secret")
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(t.TempDir(), "server.db")

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open server database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })

	jwtProvider, err := security.NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("new JWT provider: %v", err)
	}
	token, err := jwtProvider.GenerateToken("cli-sync-tester", time.Hour)
	if err != nil {
		t.Fatalf("generate token: %v", err)
	}

	srv := mcp.NewServer(database, jwtProvider, "test", cfg)
	ts := httptest.NewServer(srv.HTTPHandler())
	t.Cleanup(ts.Close)

	return &cliTestServer{db: database, token: token, url: ts.URL}
}

// newCLITestLocalDB opens a fresh local memory database at a stable path an
// returns it together with that path, so tests can close it, run the CLI
// against --db <path> and reopen the same file (proving reuse in place).
func newCLITestLocalDB(t *testing.T) (*db.DB, string) {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(t.TempDir(), "local.db")
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open local database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database, cfg.Database.Path
}

func cliStoreMemory(t *testing.T, database *db.DB, id, content string, ts time.Time) {
	t.Helper()
	m := &db.Memory{ID: id, Content: content, Scope: "global", CreatedAt: ts, UpdatedAt: ts}
	if _, err := database.UpsertMemoryIfNewer(m); err != nil {
		t.Fatalf("store memory %s: %v", id, err)
	}
}

func TestCmdMemory_NoArgsPrintsUsage(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := cmdMemory(nil, &stdout, &stderr); code != exitcodes.ExitNoInput {
		t.Fatalf("exit = %v, want %v", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "memory sync") {
		t.Errorf("stderr missing usage: %q", stderr.String())
	}
}

func TestCmdMemory_Help(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := cmdMemory([]string{"--help"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, want %v", code, exitcodes.ExitOK)
	}
	if !strings.Contains(stdout.String(), "memory sync") {
		t.Errorf("stdout missing usage: %q", stdout.String())
	}
}

func TestCmdMemory_UnknownSubcommand(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := cmdMemory([]string{"bogus"}, &stdout, &stderr); code != exitcodes.ExitNoInput {
		t.Fatalf("exit = %v, want %v", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), `unknown subcommand "bogus"`) {
		t.Errorf("stderr missing message: %q", stderr.String())
	}
}

// TestRun_MemoryIsDispatchedNotPassthrough proves the memory command enters
// the normal dispatcher (it must never become a passthrough — the embedded
// cores test in cmd_passthrough_test.go guards the same property).
func TestRun_MemoryIsDispatchedNotPassthrough(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "bogus"}, &stdout, &stderr); code != exitcodes.ExitNoInput {
		t.Fatalf("run(memory bogus) = %v, want %v", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "unknown subcommand") {
		t.Errorf("stderr missing dispatcher message: %q", stderr.String())
	}
	if !strings.Contains(stderr.String(), "memory") {
		t.Errorf("stderr missing command context: %q", stderr.String())
	}
}

func TestCmdMemorySync_Validation(t *testing.T) {
	tests := []struct {
		name    string
		args    []string
		wantErr string
	}{
		{"missing remote", []string{"--pull"}, "--remote is required"},
		{"unexpected argument", []string{"--remote", "http://x", "extra"}, "unexpected argument"},
		{"relay without passphrase", []string{"--remote", "http://x", "--encrypted-relay"}, "--relay-passphrase"},
		{"invalid output format", []string{"--remote", "http://x", "--output", "xml"}, "unsupported output format"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := cmdMemorySync(tt.args, &stdout, &stderr)
			if code != exitcodes.ExitNoInput {
				t.Fatalf("exit = %v, want %v (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
			}
			if !strings.Contains(stderr.String(), tt.wantErr) {
				t.Errorf("stderr %q does not contain %q", stderr.String(), tt.wantErr)
			}
		})
	}
}

func TestCmdMemorySync_PullOnly(t *testing.T) {
	server := newCLITestServer(t)
	local, localPath := newCLITestLocalDB(t)

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, server.db, remoteID, "remote fact", base.Add(2*time.Second))
	cliStoreMemory(t, local, localID, "local fact", base.Add(time.Second))
	if err := local.Close(); err != nil {
		t.Fatalf("close local db: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{"--remote", server.url, "--token", server.token,
		"--pull", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr: %s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "Pulled memories") {
		t.Errorf("table output missing counters: %q", stdout.String())
	}

	reopened, err := db.Open(&config.Config{Database: config.DatabaseConfig{Path: localPath}})
	if err != nil {
		t.Fatalf("reopen local db: %v", err)
	}
	defer func() { _ = reopened.Close() }()

	got, err := reopened.GetMemory(remoteID)
	if err != nil || got == nil || got.Content != "remote fact" {
		t.Errorf("local db missing pulled memory: %+v err=%v", got, err)
	}
	if m, _ := reopened.GetMemory(localID); m == nil {
		t.Error("pull-only must not remove existing local memories")
	}
	if m, _ := server.db.GetMemory(localID); m != nil {
		t.Error("pull-only must not push the local memory to the server")
	}
}

func TestCmdMemorySync_PushOnly(t *testing.T) {
	server := newCLITestServer(t)
	local, localPath := newCLITestLocalDB(t)

	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, local, localID, "local fact", base.Add(time.Second))
	if err := local.Close(); err != nil {
		t.Fatalf("close local db: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{"--remote", server.url, "--token", server.token,
		"--push", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr: %s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "Pushed memories") {
		t.Errorf("table output missing counters: %q", stdout.String())
	}

	got, err := server.db.GetMemory(localID)
	if err != nil || got == nil || got.Content != "local fact" {
		t.Errorf("server db missing pushed memory: %+v err=%v", got, err)
	}
}

func TestCmdMemorySync_BidirectionalWithDelete(t *testing.T) {
	server := newCLITestServer(t)
	remote, remotePath := newCLITestLocalDB(t)

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, server.db, remoteID, "remote fact", base.Add(2*time.Second))
	cliStoreMemory(t, remote, localID, "local fact", base.Add(time.Second))
	if err := remote.Close(); err != nil {
		t.Fatalf("close remote-side db: %v", err)
	}

	runCLISync := func(args ...string) (string, string) {
		t.Helper()
		var stdout, stderr bytes.Buffer
		code := cmdMemorySync(append([]string{"--remote", server.url, "--token", server.token, "--db", remotePath}, args...), &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr: %s", code, stderr.String())
		}
		return stdout.String(), stderr.String()
	}

	// Default (no --pull/--push) runs both directions.
	out, _ := runCLISync()
	if !strings.Contains(out, "Mode") {
		t.Errorf("table output missing Mode row: %q", out)
	}
	reopened, err := db.Open(&config.Config{Database: config.DatabaseConfig{Path: remotePath}})
	if err != nil {
		t.Fatalf("reopen db: %v", err)
	}
	defer func() { _ = reopened.Close() }()

	if got, _ := reopened.GetMemory(remoteID); got == nil || got.Content != "remote fact" {
		t.Errorf("local db missing pulled memory: %+v", got)
	}
	if got, _ := server.db.GetMemory(localID); got == nil || got.Content != "local fact" {
		t.Errorf("server db missing pushed memory: %+v", got)
	}

	// Delete on the server; the next (default both) run propagates it.
	if err := server.db.DeleteMemory(remoteID); err != nil {
		t.Fatalf("server delete: %v", err)
	}
	if err := reopened.Close(); err != nil {
		t.Fatalf("close before second sync: %v", err)
	}
	_, _ = runCLISync()
	reopened2, err := db.Open(&config.Config{Database: config.DatabaseConfig{Path: remotePath}})
	if err != nil {
		t.Fatalf("reopen db after delete: %v", err)
	}
	defer func() { _ = reopened2.Close() }()
	if m, _ := reopened2.GetMemory(remoteID); m != nil {
		t.Error("remote delete not propagated to the local db")
	}

	// The per-remote sync cursor was persisted in the reused database.
	cursor, err := reopened2.GetSyncCursor(server.url)
	if err != nil {
		t.Fatalf("GetSyncCursor: %v", err)
	}
	if cursor.IsZero() {
		t.Error("sync cursor not persisted")
	}
}

func TestCmdMemorySync_TokenRequired(t *testing.T) {
	server := newCLITestServer(t)
	local, localPath := newCLITestLocalDB(t)
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, server.db, uuid.NewString(), "protected", base.Add(time.Second))
	if err := local.Close(); err != nil {
		t.Fatalf("close local db: %v", err)
	}

	// Without a token the sync must fail loudly.
	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{"--remote", server.url, "--pull", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitGeneric {
		t.Fatalf("exit without token = %v, want %v (stderr: %s)", code, exitcodes.ExitGeneric, stderr.String())
	}
	if !strings.Contains(stderr.String(), "memory sync:") {
		t.Errorf("stderr missing error context: %q", stderr.String())
	}

	// A wrong token fails the same way.
	stderr.Reset()
	code = cmdMemorySync([]string{"--remote", server.url, "--token", "wrong", "--pull", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitGeneric {
		t.Fatalf("exit with wrong token = %v, want %v", code, exitcodes.ExitGeneric)
	}

	// The correct token succeeds.
	stderr.Reset()
	code = cmdMemorySync([]string{"--remote", server.url, "--token", server.token, "--pull", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit with token = %v (stderr: %s)", code, stderr.String())
	}
}

func TestCmdMemorySync_EncryptedRelay(t *testing.T) {
	server := newCLITestServer(t)
	local, localPath := newCLITestLocalDB(t)

	const passphrase = "cli-relay-passphrase"
	const secretContent = "relay must not see this plaintext"
	id := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, local, id, secretContent, base.Add(time.Second))
	if err := local.Close(); err != nil {
		t.Fatalf("close local db: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{"--remote", server.url, "--token", server.token,
		"--encrypted-relay", "--relay-passphrase", passphrase, "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr: %s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "Relay blobs stored") {
		t.Errorf("table output missing relay row: %q", stdout.String())
	}

	// The relay holds ciphertext only.
	blobs, err := server.db.GetRelayBlobsSince(time.Time{}, 1000)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince: %v", err)
	}
	if len(blobs) != 1 {
		t.Fatalf("relay blobs = %d, want 1", len(blobs))
	}
	if bytes.Contains(blobs[0].Blob, []byte(secretContent)) {
		t.Error("relay blob contains plaintext")
	}

	// A second local database pulls and decrypts the blob.
	receiver, receiverPath := newCLITestLocalDB(t)
	if err := receiver.Close(); err != nil {
		t.Fatalf("close receiver db: %v", err)
	}
	stdout.Reset()
	stderr.Reset()
	code = cmdMemorySync([]string{"--remote", server.url, "--token", server.token,
		"--pull", "--encrypted-relay", "--relay-passphrase", passphrase, "--db", receiverPath, "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("receiver exit = %v, stderr: %s", code, stderr.String())
	}
	var res syncclient.Result
	if err := json.Unmarshal(stdout.Bytes(), &res); err != nil {
		t.Fatalf("decode --json output: %v (%q)", err, stdout.String())
	}
	if res.RelayFetched != 1 || res.PulledMemories != 1 {
		t.Errorf("json result = %+v", res)
	}
	if !res.EncryptedRelay {
		t.Error("json result missing encrypted_relay marker")
	}
	receiverReopened, err := db.Open(&config.Config{Database: config.DatabaseConfig{Path: receiverPath}})
	if err != nil {
		t.Fatalf("reopen receiver db: %v", err)
	}
	defer func() { _ = receiverReopened.Close() }()
	if got, _ := receiverReopened.GetMemory(id); got == nil || got.Content != secretContent {
		t.Errorf("receiver did not decrypt memory: %+v", got)
	}

	// A wrong passphrase fails the run with a clear error.
	wrong, wrongPath := newCLITestLocalDB(t)
	if err := wrong.Close(); err != nil {
		t.Fatalf("close wrong-pass db: %v", err)
	}
	stderr.Reset()
	code = cmdMemorySync([]string{"--remote", server.url, "--token", server.token,
		"--pull", "--encrypted-relay", "--relay-passphrase", "wrong", "--db", wrongPath}, &stdout, &stderr)
	if code != exitcodes.ExitGeneric {
		t.Fatalf("wrong-passphrase exit = %v, want %v", code, exitcodes.ExitGeneric)
	}
	if !strings.Contains(stderr.String(), "decrypt relay blob") {
		t.Errorf("stderr missing decrypt error: %q", stderr.String())
	}
}

func TestCmdMemorySync_TokenEnvFallback(t *testing.T) {
	server := newCLITestServer(t)
	local, localPath := newCLITestLocalDB(t)
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	cliStoreMemory(t, server.db, uuid.NewString(), "env token works", base.Add(time.Second))
	if err := local.Close(); err != nil {
		t.Fatalf("close local db: %v", err)
	}

	t.Setenv("SYMBRAIN_MEMORY_SYNC_TOKEN", server.token)
	var stdout, stderr bytes.Buffer
	code := cmdMemorySync([]string{"--remote", server.url, "--pull", "--db", localPath}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("exit = %v, stderr: %s", code, stderr.String())
	}
}
