package db

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func oplogTestDB(t *testing.T) *DB {
	t.Helper()
	tempDir := t.TempDir()
	t.Setenv("HOME", tempDir)
	t.Setenv("XDG_DATA_HOME", tempDir)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open test database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func oplogMemory(id, content string) *Memory {
	now := time.Now().UTC()
	return &Memory{
		ID:        id,
		Content:   content,
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func TestOplogRecordsUpsertAndDelete(t *testing.T) {
	database := oplogTestDB(t)
	start := time.Now().UTC().Add(-1 * time.Minute)

	m := oplogMemory("11111111-1111-1111-1111-111111111111", "fact one")
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("save failed: %v", err)
	}

	deleted, err := database.GetDeletedSince(start)
	if err != nil {
		t.Fatalf("GetDeletedSince failed: %v", err)
	}
	if len(deleted) != 0 {
		t.Fatalf("expected no tombstones after insert, got %d", len(deleted))
	}

	if err := database.DeleteMemory(m.ID); err != nil {
		t.Fatalf("delete failed: %v", err)
	}

	deleted, err = database.GetDeletedSince(start)
	if err != nil {
		t.Fatalf("GetDeletedSince failed: %v", err)
	}
	if len(deleted) != 1 || deleted[0].ID != m.ID {
		t.Fatalf("expected one tombstone for %s, got %+v", m.ID, deleted)
	}
	if deleted[0].DeletedAt.IsZero() {
		t.Fatal("tombstone timestamp must not be zero")
	}
}

func TestOplogRecreatedMemoryIsNotReportedDeleted(t *testing.T) {
	database := oplogTestDB(t)
	start := time.Now().UTC().Add(-1 * time.Minute)

	m := oplogMemory("22222222-2222-2222-2222-222222222222", "fact two")
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("save failed: %v", err)
	}
	if err := database.DeleteMemory(m.ID); err != nil {
		t.Fatalf("delete failed: %v", err)
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("re-save failed: %v", err)
	}

	deleted, err := database.GetDeletedSince(start)
	if err != nil {
		t.Fatalf("GetDeletedSince failed: %v", err)
	}
	if len(deleted) != 0 {
		t.Fatalf("re-created memory must not appear as deleted, got %+v", deleted)
	}
}

func TestSyncUpsertDoesNotResurrectTombstonedMemory(t *testing.T) {
	database := oplogTestDB(t)

	m := oplogMemory("33333333-3333-3333-3333-333333333333", "fact three")
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("save failed: %v", err)
	}
	if err := database.DeleteMemory(m.ID); err != nil {
		t.Fatalf("delete failed: %v", err)
	}

	// A stale remote copy (updated before the local delete) must be skipped.
	stale := oplogMemory(m.ID, "stale remote copy")
	stale.UpdatedAt = time.Now().UTC().Add(-1 * time.Hour)
	applied, err := database.SyncUpsertMemoryIfNewer(stale)
	if err != nil {
		t.Fatalf("SyncUpsertMemoryIfNewer failed: %v", err)
	}
	if applied {
		t.Fatal("stale remote copy must not resurrect a deleted memory")
	}

	// A copy updated after the delete wins (the remote edit is newer).
	fresh := oplogMemory(m.ID, "fresh remote edit")
	fresh.UpdatedAt = time.Now().UTC().Add(1 * time.Hour)
	applied, err = database.SyncUpsertMemoryIfNewer(fresh)
	if err != nil {
		t.Fatalf("SyncUpsertMemoryIfNewer failed: %v", err)
	}
	if !applied {
		t.Fatal("a remote edit newer than the tombstone must be applied")
	}
}

func TestApplyRemoteDeleteLWW(t *testing.T) {
	database := oplogTestDB(t)

	m := oplogMemory("44444444-4444-4444-4444-444444444444", "fact four")
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("save failed: %v", err)
	}

	// Tombstone older than the local row: local edit wins, no delete.
	removed, err := database.ApplyRemoteDelete(m.ID, m.UpdatedAt.Add(-1*time.Hour))
	if err != nil {
		t.Fatalf("ApplyRemoteDelete failed: %v", err)
	}
	if removed {
		t.Fatal("older tombstone must not delete a newer local row")
	}

	// Tombstone newer than the local row: delete applies.
	removed, err = database.ApplyRemoteDelete(m.ID, m.UpdatedAt.Add(1*time.Hour))
	if err != nil {
		t.Fatalf("ApplyRemoteDelete failed: %v", err)
	}
	if !removed {
		t.Fatal("newer tombstone must delete the local row")
	}
	got, err := database.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Fatal("memory must be gone after remote delete")
	}
}

func TestRelayBlobLWWAndListing(t *testing.T) {
	database := oplogTestDB(t)
	base := time.Now().UTC()

	stored, err := database.StoreRelayBlob(RelayBlob{ID: "blob-1", UpdatedAt: base, Blob: []byte("cipher-v1")})
	if err != nil || !stored {
		t.Fatalf("first store failed: stored=%v err=%v", stored, err)
	}

	// Older write is skipped.
	stored, err = database.StoreRelayBlob(RelayBlob{ID: "blob-1", UpdatedAt: base.Add(-1 * time.Minute), Blob: []byte("older")})
	if err != nil {
		t.Fatalf("store failed: %v", err)
	}
	if stored {
		t.Fatal("older blob must not overwrite a newer one")
	}

	// Newer write overwrites.
	stored, err = database.StoreRelayBlob(RelayBlob{ID: "blob-1", UpdatedAt: base.Add(1 * time.Minute), Blob: []byte("cipher-v2")})
	if err != nil || !stored {
		t.Fatalf("newer store failed: stored=%v err=%v", stored, err)
	}

	blobs, err := database.GetRelayBlobsSince(base.Add(-1*time.Hour), 0)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince failed: %v", err)
	}
	if len(blobs) != 1 || string(blobs[0].Blob) != "cipher-v2" {
		t.Fatalf("expected single blob cipher-v2, got %+v", blobs)
	}

	blobs, err = database.GetRelayBlobsSince(base.Add(2*time.Minute), 0)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince failed: %v", err)
	}
	if len(blobs) != 0 {
		t.Fatalf("cursor past the blob must return nothing, got %+v", blobs)
	}
}

// testEmbedding builds a valid db.EmbeddingDim-sized vector with the given
// leading values (dimension mismatches are now a handled error).
func testEmbedding(leading ...float32) []float32 {
	v := make([]float32, EmbeddingDim)
	copy(v, leading)
	return v
}
