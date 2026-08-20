package mcp

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

func serviceTestMemory(id, content string) *db.Memory {
	now := time.Now().UTC()
	return &db.Memory{
		ID:        id,
		Content:   content,
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: testEmbedding(0.1),
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func TestMemoryService_Set(t *testing.T) {
	s := helperServer(t)

	id, err := s.service.Set("hello world", "global", nil, "sess-1", "tester", nil, "test-tool", false, 0)
	if err != nil {
		t.Fatalf("Set failed: %v", err)
	}
	if id == "" {
		t.Fatal("expected non-empty memory id")
	}

	got, err := s.service.db.GetMemory(id)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil || got.Content != "hello world" {
		t.Fatalf("expected stored content 'hello world', got %+v", got)
	}
}

func TestMemoryService_UpsertMemoryIfNewer_RedactsWhenPIIEnabled(t *testing.T) {
	s := helperServer(t)
	s.service.SetPIIEnabled(true)

	m := serviceTestMemory("22222222-2222-2222-2222-222222222222", "my email is test@example.com")
	ok, err := s.service.UpsertMemoryIfNewer(m)
	if err != nil {
		t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
	}
	if !ok {
		t.Fatal("expected upsert to report a change")
	}

	stored, err := s.service.db.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if stored.Content == "my email is test@example.com" {
		t.Errorf("expected PII to be redacted, got raw content: %q", stored.Content)
	}
	if stored.Content != security.Redact("my email is test@example.com") {
		t.Errorf("expected redacted content to match security.Redact output, got %q", stored.Content)
	}
}

func TestMemoryService_SyncUpsertMemoryIfNewer_SkipsResurrectionAfterTombstone(t *testing.T) {
	s := helperServer(t)

	m := serviceTestMemory("33333333-3333-3333-3333-333333333333", "will be deleted")
	if err := s.service.db.SaveMemory(m); err != nil {
		t.Fatalf("seed SaveMemory failed: %v", err)
	}
	if err := s.service.db.DeleteMemory(m.ID); err != nil {
		t.Fatalf("DeleteMemory failed: %v", err)
	}

	// An incoming row older than the tombstone must not resurrect the memory.
	stale := serviceTestMemory(m.ID, "resurrected content")
	stale.UpdatedAt = m.UpdatedAt.Add(-1 * time.Minute)
	ok, err := s.service.SyncUpsertMemoryIfNewer(stale)
	if err != nil {
		t.Fatalf("SyncUpsertMemoryIfNewer failed: %v", err)
	}
	if ok {
		t.Error("expected stale upsert to be rejected by the tombstone")
	}

	got, err := s.service.db.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Errorf("expected memory to remain deleted, got %+v", got)
	}
}

func TestMemoryService_DeletedSinceAndApplyRemoteDelete(t *testing.T) {
	s := helperServer(t)
	start := time.Now().UTC().Add(-1 * time.Minute)

	m := serviceTestMemory("44444444-4444-4444-4444-444444444444", "to be remotely deleted")
	if err := s.service.db.SaveMemory(m); err != nil {
		t.Fatalf("seed SaveMemory failed: %v", err)
	}

	deletedAt := time.Now().UTC().Add(1 * time.Minute)
	applied, err := s.service.ApplyRemoteDelete(m.ID, deletedAt)
	if err != nil {
		t.Fatalf("ApplyRemoteDelete failed: %v", err)
	}
	if !applied {
		t.Fatal("expected remote delete to be applied")
	}

	deleted, err := s.service.GetDeletedSince(start)
	if err != nil {
		t.Fatalf("GetDeletedSince failed: %v", err)
	}
	found := false
	for _, d := range deleted {
		if d.ID == m.ID {
			found = true
		}
	}
	if !found {
		t.Errorf("expected %s in GetDeletedSince results, got %+v", m.ID, deleted)
	}
}

func TestMemoryService_RelayBlobRoundTrip(t *testing.T) {
	s := helperServer(t)
	start := time.Now().UTC().Add(-1 * time.Minute)

	blob := db.RelayBlob{ID: "relay-1", UpdatedAt: time.Now().UTC(), Blob: []byte("ciphertext")}
	stored, err := s.service.StoreRelayBlob(blob)
	if err != nil {
		t.Fatalf("StoreRelayBlob failed: %v", err)
	}
	if !stored {
		t.Fatal("expected blob to be stored")
	}

	blobs, err := s.service.GetRelayBlobsSince(start, 10)
	if err != nil {
		t.Fatalf("GetRelayBlobsSince failed: %v", err)
	}
	if len(blobs) != 1 || blobs[0].ID != "relay-1" {
		t.Fatalf("expected one relay blob with id relay-1, got %+v", blobs)
	}

	// An older write must not overwrite the newer stored blob.
	older := db.RelayBlob{ID: "relay-1", UpdatedAt: blob.UpdatedAt.Add(-1 * time.Minute), Blob: []byte("stale")}
	overwritten, err := s.service.StoreRelayBlob(older)
	if err != nil {
		t.Fatalf("StoreRelayBlob (stale) failed: %v", err)
	}
	if overwritten {
		t.Error("expected stale relay blob write to be rejected")
	}
}

func TestMemoryService_GraphNeighborsAsOf(t *testing.T) {
	s := helperServer(t)

	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-alice", Name: "Alice", Type: "person"}); err != nil {
		t.Fatalf("seed Alice: %v", err)
	}
	if err := s.service.db.SaveEntity(&db.Entity{ID: "e-bob", Name: "Bob", Type: "person"}); err != nil {
		t.Fatalf("seed Bob: %v", err)
	}

	past := time.Now().UTC().Add(-1 * time.Hour)
	if err := s.service.db.SaveEntityRelation(&db.EntityRelation{
		FromEntityID: "e-alice",
		ToEntityID:   "e-bob",
		RelationType: "works-with",
		ValidFrom:    &past,
	}); err != nil {
		t.Fatalf("SaveEntityRelation: %v", err)
	}

	entities, relations, err := s.service.GraphNeighborsAsOf("e-alice", 1, nil)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf failed: %v", err)
	}
	if len(relations) != 1 || relations[0].ToEntityID != "e-bob" {
		t.Fatalf("expected one relation to Bob, got %+v", relations)
	}
	found := false
	for _, e := range entities {
		if e.ID == "e-bob" {
			found = true
		}
	}
	if !found {
		t.Errorf("expected Bob among neighbor entities, got %+v", entities)
	}

	// A cutoff before the relation's validity window must exclude it.
	before := past.Add(-1 * time.Hour)
	_, relationsBefore, err := s.service.GraphNeighborsAsOf("e-alice", 1, &before)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (before) failed: %v", err)
	}
	if len(relationsBefore) != 0 {
		t.Errorf("expected no relations before validity window, got %+v", relationsBefore)
	}
}

func TestNotFoundError(t *testing.T) {
	err := &NotFoundError{Resource: "memory", Identifier: "abc-123"}
	want := "memory not found: abc-123"
	if err.Error() != want {
		t.Errorf("expected %q, got %q", want, err.Error())
	}
}

// testEmbedding builds a valid db.EmbeddingDim-sized vector with the given
// leading values (dimension mismatches are now a handled error).
func testEmbedding(leading ...float32) []float32 {
	v := make([]float32, db.EmbeddingDim)
	copy(v, leading)
	return v
}
