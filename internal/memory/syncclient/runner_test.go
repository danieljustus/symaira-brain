package syncclient

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/google/uuid"
)

// testPeer is a full in-process memory server (same construction the mcp
// package's own tests use) serving the HTTP API on a random localhost port.
type testPeer struct {
	db    *db.DB
	token string
	url   string
}

// newTestPeer opens a fresh memory database and wraps it in a real memory
// HTTP server. The shared JWT secret comes from the environment, which is
// set once per test via t.Setenv(JWT_SECRET_KEY, ...).
func newTestPeer(t *testing.T, name string) *testPeer {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(t.TempDir(), name+".db")

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open %s database: %v", name, err)
	}
	t.Cleanup(func() { _ = database.Close() })

	jwtProvider, err := security.NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("new JWT provider: %v", err)
	}
	token, err := jwtProvider.GenerateToken("sync-tester", time.Hour)
	if err != nil {
		t.Fatalf("generate token: %v", err)
	}

	srv := mcp.NewServer(database, jwtProvider, "test", cfg)
	ts := httptest.NewServer(srv.HTTPHandler())
	t.Cleanup(ts.Close)

	return &testPeer{db: database, token: token, url: ts.URL}
}

// storeMemory writes a memory with deterministic timestamps so LWW behavior
// is reproducible.
func storeMemory(t *testing.T, database *db.DB, id, content string, ts time.Time) {
	t.Helper()
	m := &db.Memory{
		ID:        id,
		Content:   content,
		Scope:     "global",
		CreatedAt: ts,
		UpdatedAt: ts,
	}
	if _, err := database.UpsertMemoryIfNewer(m); err != nil {
		t.Fatalf("store memory %s: %v", id, err)
	}
}

func getMemory(t *testing.T, database *db.DB, id string) *db.Memory {
	t.Helper()
	m, err := database.GetMemory(id)
	if err != nil {
		t.Fatalf("get memory %s: %v", id, err)
	}
	return m
}

// memoryExists reports whether a memory row is present. GetMemory returns
// (nil, nil) for missing rows, so absence must be checked via nil, not error.
func memoryExists(t *testing.T, database *db.DB, id string) bool {
	t.Helper()
	m, err := database.GetMemory(id)
	if err != nil {
		t.Fatalf("get memory %s: %v", id, err)
	}
	return m != nil
}

func TestRun_BidirectionalPlain(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, remote.db, remoteID, "fact living on the remote", base.Add(2*time.Second))
	storeMemory(t, local.db, localID, "fact living locally", base.Add(1*time.Second))

	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, Push: true, DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.Mode != "both" {
		t.Errorf("mode = %q", res.Mode)
	}
	if res.PulledMemories != 1 {
		t.Errorf("pulled memories = %d, want 1", res.PulledMemories)
	}
	if res.PushedMemories != 1 {
		t.Errorf("pushed memories = %d, want 1", res.PushedMemories)
	}
	if !res.Cursor.After(base) {
		t.Errorf("cursor %v not advanced past %v", res.Cursor, base)
	}

	if got := getMemory(t, local.db, remoteID); got.Content != "fact living on the remote" {
		t.Errorf("local missing remote memory: %+v", got)
	}
	if got := getMemory(t, remote.db, localID); got.Content != "fact living locally" {
		t.Errorf("remote missing local memory: %+v", got)
	}

	// The stored cursor survives: a second run must not re-apply anything.
	cursor, err := local.db.GetSyncCursor(remote.url)
	if err != nil {
		t.Fatalf("GetSyncCursor: %v", err)
	}
	if !cursor.Equal(res.Cursor) {
		t.Errorf("stored cursor = %v, want %v", cursor, res.Cursor)
	}
	res2, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, Push: true, DB: local.db,
	})
	if err != nil {
		t.Fatalf("second Run: %v", err)
	}
	if res2.PulledMemories != 0 || res2.PushedMemories != 0 {
		t.Errorf("second run applied changes: %+v", res2)
	}
}

func TestRun_PullOnly(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, remote.db, remoteID, "remote-only", base.Add(2*time.Second))
	storeMemory(t, local.db, localID, "local-only", base.Add(1*time.Second))

	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, Push: false, DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.Mode != "pull" {
		t.Errorf("mode = %q", res.Mode)
	}
	if res.PulledMemories != 1 {
		t.Errorf("pulled = %d, want 1", res.PulledMemories)
	}
	if res.PushedMemories != 0 {
		t.Errorf("pushed = %d, want 0", res.PushedMemories)
	}
	if got := getMemory(t, local.db, remoteID); got.Content != "remote-only" {
		t.Errorf("local missing remote memory: %+v", got)
	}
	if memoryExists(t, remote.db, localID) {
		t.Error("pull-only must not push the local memory")
	}
}

func TestRun_PushOnly(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	remoteID := uuid.NewString()
	localID := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, remote.db, remoteID, "remote-only", base.Add(2*time.Second))
	storeMemory(t, local.db, localID, "local-only", base.Add(1*time.Second))

	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: false, Push: true, DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.Mode != "push" {
		t.Errorf("mode = %q", res.Mode)
	}
	if res.PushedMemories != 1 {
		t.Errorf("pushed = %d, want 1", res.PushedMemories)
	}
	if memoryExists(t, local.db, remoteID) {
		t.Error("push-only must not pull the remote memory")
	}
	if got := getMemory(t, remote.db, localID); got.Content != "local-only" {
		t.Errorf("remote missing local memory: %+v", got)
	}
}

func TestRun_DeletePropagation(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	id := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, remote.db, id, "to be deleted", base.Add(time.Second))

	if _, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, Push: true, DB: local.db,
	}); err != nil {
		t.Fatalf("initial Run: %v", err)
	}
	if got := getMemory(t, local.db, id); got == nil || got.Content != "to be deleted" {
		t.Fatalf("pull did not deliver the memory: %+v", got)
	}

	// Delete on the remote; the tombstone must reach the local side.
	if err := remote.db.DeleteMemory(id); err != nil {
		t.Fatalf("remote delete: %v", err)
	}
	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, Push: true, DB: local.db,
	})
	if err != nil {
		t.Fatalf("second Run: %v", err)
	}
	if res.PulledDeletes != 1 {
		t.Errorf("pulled deletes = %d, want 1", res.PulledDeletes)
	}
	if memoryExists(t, local.db, id) {
		t.Error("local memory not removed by remote tombstone")
	}
}

func TestRun_EncryptedRelay(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	relayHost := newTestPeer(t, "relay")
	local := newTestPeer(t, "local")

	const passphrase = "test-relay-passphrase"
	const secretContent = "client-side secret that the relay must never see"
	id := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, local.db, id, secretContent, base.Add(time.Second))

	// Push the encrypted blob to the relay, then pull it back.
	res, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, Push: true, EncryptedRelay: true, Passphrase: passphrase, DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.RelayStored != 1 {
		t.Errorf("relay stored = %d, want 1", res.RelayStored)
	}

	// The relay host stores ciphertext only — content must not appear.
	blobs, err := relayHost.db.GetRelayBlobsSince(time.Time{}, 1000)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince: %v", err)
	}
	if len(blobs) != 1 || blobs[0].ID != id {
		t.Fatalf("relay blobs = %+v", blobs)
	}
	if bytes.Contains(blobs[0].Blob, []byte(secretContent)) {
		t.Error("relay blob contains plaintext memory content")
	}
	if strings.Contains(string(blobs[0].Blob), secretContent) {
		t.Error("relay blob string contains plaintext memory content")
	}

	// The relay host's own memory table must stay empty (it is only a relay).
	if memoryExists(t, relayHost.db, id) {
		t.Error("relay host stored plaintext memory")
	}

	// A second peer (receiver) pulls the blob and decrypts it locally.
	receiver := newTestPeer(t, "receiver")
	res2, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, Push: false, EncryptedRelay: true, Passphrase: passphrase, DB: receiver.db,
	})
	if err != nil {
		t.Fatalf("receiver Run: %v", err)
	}
	if res2.RelayFetched != 1 || res2.PulledMemories != 1 {
		t.Errorf("receiver counters = pulled:%d fetched:%d", res2.PulledMemories, res2.RelayFetched)
	}
	if got := getMemory(t, receiver.db, id); got.Content != secretContent {
		t.Errorf("receiver did not decrypt memory: %+v", got)
	}

	// A wrong passphrase must fail loudly, not corrupt the database.
	wrongPeer := newTestPeer(t, "wrong-pass")
	_, err = Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, EncryptedRelay: true, Passphrase: "wrong-passphrase", DB: wrongPeer.db,
	})
	if err == nil {
		t.Fatal("expected decrypt failure with wrong passphrase")
	}
	if !strings.Contains(err.Error(), "decrypt relay blob") {
		t.Errorf("error %q lacks decrypt context", err)
	}
}

func TestRun_RelayDeleteTombstone(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	relayHost := newTestPeer(t, "relay")
	publisher := newTestPeer(t, "publisher")
	receiver := newTestPeer(t, "receiver")

	const passphrase = "relay-delete-passphrase"
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	id := uuid.NewString()
	storeMemory(t, publisher.db, id, "ephemeral", base.Add(time.Second))
	if _, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, Push: true, EncryptedRelay: true, Passphrase: passphrase, DB: publisher.db,
	}); err != nil {
		t.Fatalf("publish Run: %v", err)
	}

	if err := publisher.db.DeleteMemory(id); err != nil {
		t.Fatalf("delete: %v", err)
	}
	// Push again with delete-older-than timestamps: tombstone blob created.
	if _, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, Push: true, EncryptedRelay: true, Passphrase: passphrase, DB: publisher.db,
	}); err != nil {
		t.Fatalf("tombstone Run: %v", err)
	}

	// Receiver pulls: applies the memory, then the tombstone removes it.
	if _, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token,
		Pull: true, Push: false, EncryptedRelay: true, Passphrase: passphrase, DB: receiver.db,
	}); err != nil {
		t.Fatalf("receiver Run: %v", err)
	}
	if memoryExists(t, receiver.db, id) {
		t.Error("tombstone did not remove the memory on the receiver")
	}
}

func TestRun_TokenAuth(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	id := uuid.NewString()
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	storeMemory(t, remote.db, id, "protected", base.Add(time.Second))

	// Without a token the API is closed: pull must fail.
	_, err := Run(context.Background(), Options{
		Remote: remote.url, Pull: true, DB: local.db,
	})
	if err == nil {
		t.Fatal("expected auth failure without token")
	}
	if !strings.Contains(err.Error(), "remote returned") {
		t.Errorf("error %q lacks remote-status context", err)
	}

	// A wrong token must fail the same way.
	_, err = Run(context.Background(), Options{
		Remote: remote.url, Token: "wrong-token", Pull: true, DB: local.db,
	})
	if err == nil {
		t.Fatal("expected auth failure with wrong token")
	}

	// The real token succeeds.
	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: true, DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run with token: %v", err)
	}
	if res.PulledMemories != 1 {
		t.Errorf("pulled = %d, want 1", res.PulledMemories)
	}
}

func TestRun_Validation(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-shared-secret")
	peer := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	tests := []struct {
		name    string
		opts    Options
		wantErr string
	}{
		{"missing remote", Options{DB: local.db, Pull: true}, "remote URL is required"},
		{"missing db", Options{Remote: peer.url, Pull: true}, "database is required"},
		{"no direction", Options{Remote: peer.url, DB: local.db}, "at least one of pull or push"},
		{"relay without passphrase", Options{Remote: peer.url, DB: local.db, Pull: true, EncryptedRelay: true}, "passphrase"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := Run(context.Background(), tt.opts)
			if err == nil {
				t.Fatal("expected error")
			}
			if !strings.Contains(err.Error(), tt.wantErr) {
				t.Errorf("error %q does not contain %q", err, tt.wantErr)
			}
		})
	}
}

// fakePushClient records Apply and RelayPush calls for batch assertions.
type fakePushClient struct {
	applyCalls       int
	relayPushCalls   int
	applyMemBatches  [][]*db.Memory
	applyDelBatches  [][]db.DeletedMemory
	relayBlobBatches [][]db.RelayBlob
	failOnApplyNth   int
	failOnRelayNth   int
	applyErr         error
	relayErr         error
}

func (f *fakePushClient) Apply(ctx context.Context, memories []*db.Memory, deleted []db.DeletedMemory) (*ApplyResult, error) {
	f.applyCalls++
	f.applyMemBatches = append(f.applyMemBatches, memories)
	f.applyDelBatches = append(f.applyDelBatches, deleted)
	if f.failOnApplyNth > 0 && f.applyCalls == f.failOnApplyNth {
		return nil, f.applyErr
	}
	return &ApplyResult{Applied: len(memories), Deleted: len(deleted)}, nil
}

func (f *fakePushClient) RelayPush(ctx context.Context, blobs []db.RelayBlob) (*RelayPushResult, error) {
	f.relayPushCalls++
	f.relayBlobBatches = append(f.relayBlobBatches, blobs)
	if f.failOnRelayNth > 0 && f.relayPushCalls == f.failOnRelayNth {
		return nil, f.relayErr
	}
	return &RelayPushResult{Stored: len(blobs)}, nil
}

func TestPlainPush_Batches(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "batch-test-secret")
	local := newTestPeer(t, "local")
	const total = 2500
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	ids := make([]string, total)
	for i := 0; i < total; i++ {
		ids[i] = uuid.NewString()
		storeMemory(t, local.db, ids[i], fmt.Sprintf("content %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	fake := &fakePushClient{}
	res := &Result{}
	_, err := plainPush(context.Background(), fake, Options{DB: local.db}, time.Time{}, res)
	if err != nil {
		t.Fatalf("plainPush: %v", err)
	}
	wantCalls := (total + pushBatchSize - 1) / pushBatchSize
	if fake.applyCalls != wantCalls {
		t.Errorf("apply calls = %d, want %d", fake.applyCalls, wantCalls)
	}
	totalSent := 0
	for i, batch := range fake.applyMemBatches {
		if len(batch) > pushBatchSize {
			t.Errorf("batch %d size = %d, max %d", i, len(batch), pushBatchSize)
		}
		totalSent += len(batch)
	}
	if totalSent != total {
		t.Errorf("total sent = %d, want %d", totalSent, total)
	}
	if res.PushedMemories != total {
		t.Errorf("pushed memories = %d, want %d", res.PushedMemories, total)
	}
}

func TestRelayPush_Batches(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "batch-test-secret")
	local := newTestPeer(t, "local")
	const total = 1100
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	ids := make([]string, total)
	for i := 0; i < total; i++ {
		ids[i] = uuid.NewString()
		storeMemory(t, local.db, ids[i], fmt.Sprintf("content %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	fake := &fakePushClient{}
	res := &Result{}
	_, err := relayPush(context.Background(), fake, Options{DB: local.db, Passphrase: "phrase"}, time.Time{}, res)
	if err != nil {
		t.Fatalf("relayPush: %v", err)
	}
	wantCalls := (total + pushBatchSize - 1) / pushBatchSize
	if fake.relayPushCalls != wantCalls {
		t.Errorf("relay push calls = %d, want %d", fake.relayPushCalls, wantCalls)
	}
	totalSent := 0
	for i, batch := range fake.relayBlobBatches {
		if len(batch) > pushBatchSize {
			t.Errorf("batch %d size = %d, max %d", i, len(batch), pushBatchSize)
		}
		totalSent += len(batch)
	}
	if totalSent != total {
		t.Errorf("total sent = %d, want %d", totalSent, total)
	}
	if res.RelayStored != total {
		t.Errorf("relay stored = %d, want %d", res.RelayStored, total)
	}
}

func TestPlainPush_ResumesAfterFailure(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "batch-resume-test")
	local := newTestPeer(t, "local")
	const total = 2500
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	ids := make([]string, total)
	for i := 0; i < total; i++ {
		ids[i] = uuid.NewString()
		storeMemory(t, local.db, ids[i], fmt.Sprintf("content %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	fake := &fakePushClient{failOnApplyNth: 1, applyErr: errors.New("simulated failure")}
	res := &Result{}
	_, err := plainPush(context.Background(), fake, Options{DB: local.db}, time.Time{}, res)
	if err == nil {
		t.Fatal("expected failure on first batch")
	}
	if fake.applyCalls != 1 {
		t.Errorf("calls before failure = %d, want 1", fake.applyCalls)
	}
	sentBeforeFailure := 0
	for _, b := range fake.applyMemBatches {
		sentBeforeFailure += len(b)
	}
	if sentBeforeFailure != pushBatchSize {
		t.Errorf("sent before failure = %d, want %d", sentBeforeFailure, pushBatchSize)
	}

	// Retry with the same cursor: only the first batch should be re-sent.
	fake2 := &fakePushClient{}
	res2 := &Result{}
	cursor, err := local.db.GetSyncCursor("")
	if err != nil {
		t.Fatalf("GetSyncCursor: %v", err)
	}
	_, err = plainPush(context.Background(), fake2, Options{DB: local.db}, cursor, res2)
	if err != nil {
		t.Fatalf("retry plainPush: %v", err)
	}
	// First retry batch is the same first batch; total calls should equal wantCalls.
	wantCalls := (total + pushBatchSize - 1) / pushBatchSize
	if fake2.applyCalls != wantCalls {
		t.Errorf("retry apply calls = %d, want %d", fake2.applyCalls, wantCalls)
	}
	totalSent := 0
	for _, b := range fake2.applyMemBatches {
		totalSent += len(b)
	}
	if totalSent != total {
		t.Errorf("retry total sent = %d, want %d", totalSent, total)
	}
	// No duplicate counters from the aborted first run.
	if res.PushedMemories != 0 {
		t.Errorf("aborted run pushed = %d, want 0", res.PushedMemories)
	}
	if res2.PushedMemories != total {
		t.Errorf("retry pushed = %d, want %d", res2.PushedMemories, total)
	}
}

func TestRelayPush_ResumesAfterFailure(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "batch-resume-test")
	local := newTestPeer(t, "local")
	const total = 1100
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	ids := make([]string, total)
	for i := 0; i < total; i++ {
		ids[i] = uuid.NewString()
		storeMemory(t, local.db, ids[i], fmt.Sprintf("content %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	fake := &fakePushClient{failOnRelayNth: 1, relayErr: errors.New("simulated failure")}
	res := &Result{}
	_, err := relayPush(context.Background(), fake, Options{DB: local.db, Passphrase: "phrase"}, time.Time{}, res)
	if err == nil {
		t.Fatal("expected failure on first batch")
	}
	if fake.relayPushCalls != 1 {
		t.Errorf("calls before failure = %d, want 1", fake.relayPushCalls)
	}

	fake2 := &fakePushClient{}
	res2 := &Result{}
	cursor, err := local.db.GetSyncCursor("")
	if err != nil {
		t.Fatalf("GetSyncCursor: %v", err)
	}
	_, err = relayPush(context.Background(), fake2, Options{DB: local.db, Passphrase: "phrase"}, cursor, res2)
	if err != nil {
		t.Fatalf("retry relayPush: %v", err)
	}
	wantCalls := (total + pushBatchSize - 1) / pushBatchSize
	if fake2.relayPushCalls != wantCalls {
		t.Errorf("retry relay calls = %d, want %d", fake2.relayPushCalls, wantCalls)
	}
	totalSent := 0
	for _, b := range fake2.relayBlobBatches {
		totalSent += len(b)
	}
	if totalSent != total {
		t.Errorf("retry total sent = %d, want %d", totalSent, total)
	}
	if res.RelayStored != 0 {
		t.Errorf("aborted run stored = %d, want 0", res.RelayStored)
	}
	if res2.RelayStored != total {
		t.Errorf("retry stored = %d, want %d", res2.RelayStored, total)
	}
}

func TestRun_BatchedPushPlain(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-batch-secret")
	remote := newTestPeer(t, "remote")
	local := newTestPeer(t, "local")

	const total = 2500
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	for i := 0; i < total; i++ {
		id := uuid.NewString()
		storeMemory(t, local.db, id, fmt.Sprintf("local %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	res, err := Run(context.Background(), Options{
		Remote: remote.url, Token: remote.token, Pull: false, Push: true, DB: local.db,
		Timeout: 5 * time.Minute,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.PushedMemories != total {
		t.Errorf("pushed memories = %d, want %d", res.PushedMemories, total)
	}
}

func TestRun_BatchedPushRelay(t *testing.T) {
	t.Setenv("JWT_SECRET_KEY", "runner-test-batch-secret")
	relayHost := newTestPeer(t, "relay")
	local := newTestPeer(t, "local")

	const total = 100
	base := time.Date(2026, 8, 20, 10, 0, 0, 0, time.UTC)
	for i := 0; i < total; i++ {
		id := uuid.NewString()
		storeMemory(t, local.db, id, fmt.Sprintf("relay %d", i), base.Add(time.Duration(i)*time.Millisecond))
	}

	res, err := Run(context.Background(), Options{
		Remote: relayHost.url, Token: relayHost.token, Pull: false, Push: true,
		EncryptedRelay: true, Passphrase: "batch-relay-pass", DB: local.db,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if res.RelayStored != total {
		t.Errorf("relay stored = %d, want %d", res.RelayStored, total)
	}
	// The relay stores ciphertext blobs; plaintext must not appear.
	blobs, err := relayHost.db.GetRelayBlobsSince(time.Time{}, 10000)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince: %v", err)
	}
	if len(blobs) != total {
		t.Errorf("relay blobs = %d, want %d", len(blobs), total)
	}
}
