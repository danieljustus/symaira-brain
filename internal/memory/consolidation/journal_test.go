package consolidation

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func newEngineTestDB(t *testing.T) (*db.DB, *Engine) {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(t.TempDir(), "test.db")
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	eng := NewEngine(database, embeddings, "", "", "", false, "chat", 0)
	return database, eng
}

// TestUndoLastConsolidationRestoresArchivedMemories covers the happy path:
// a journaled run with one consolidated (new) memory, one archived original
// and one replaced pair is fully reversed — new memory deleted, original
// re-activated, evidence reparented, run marked undone.
func TestUndoLastConsolidationRestoresArchivedMemories(t *testing.T) {
	database, eng := newEngineTestDB(t)

	// Seed the archived original and the consolidated replacement.
	original := &db.Memory{
		ID: "orig-1", Content: "original fact", Scope: "global",
		ConsolidationStatus: "archived", Metadata: map[string]string{},
	}
	replacement := &db.Memory{
		ID: "new-1", Content: "consolidated fact", Scope: "global",
		ConsolidationStatus: "raw", Metadata: map[string]string{},
	}
	if err := database.SaveMemory(original); err != nil {
		t.Fatalf("seed original: %v", err)
	}
	if err := database.SaveMemory(replacement); err != nil {
		t.Fatalf("seed replacement: %v", err)
	}

	// Journal a completed run referencing both.
	rs := &db.RunSummary{
		NewMemoryIDs:  []string{"new-1"},
		ArchivedIDs:   []string{"orig-1"},
		ReplacedToNew: map[string]string{"orig-1": "new-1"},
	}
	summaryJSON, err := db.MarshalRunSummary(rs)
	if err != nil {
		t.Fatalf("marshal summary: %v", err)
	}
	run := &db.ConsolidationRun{
		ID: "run-undo-test", RunAt: time.Now().UTC(), Status: "completed", SummaryJSON: summaryJSON,
		TotalArchived: 1, TotalConsolidated: 1,
	}
	if err := database.SaveConsolidationRun(run); err != nil {
		t.Fatalf("save run: %v", err)
	}

	summary, err := eng.UndoLastConsolidation(context.Background())
	if err != nil {
		t.Fatalf("UndoLastConsolidation: %v", err)
	}
	if summary == "" {
		t.Error("expected non-empty undo summary")
	}

	// The consolidated memory must be hard-deleted (GetMemory returns nil,nil).
	deleted, err := database.GetMemory("new-1")
	if err != nil || deleted != nil {
		t.Errorf("expected consolidated memory new-1 to be deleted, got %+v (err=%v)", deleted, err)
	}

	// The archived original must be re-activated (status raw, no consolidated_into).
	restored, err := database.GetMemory("orig-1")
	if err != nil {
		t.Fatalf("get restored original: %v", err)
	}
	if restored.ConsolidationStatus != "raw" {
		t.Errorf("expected status raw, got %q", restored.ConsolidationStatus)
	}
	if restored.ConsolidatedIntoID != "" {
		t.Errorf("expected consolidated_into_id cleared, got %q", restored.ConsolidatedIntoID)
	}

	// The run must be marked undone.
	latest, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if latest != nil && latest.Status == "completed" {
		t.Errorf("expected run to be marked undone, got status %q", latest.Status)
	}
}

// TestUndoLastConsolidationNoRun covers the error path: no journaled run
// means nothing to undo.
func TestUndoLastConsolidationNoRun(t *testing.T) {
	_, eng := newEngineTestDB(t)
	_, err := eng.UndoLastConsolidation(context.Background())
	if err == nil {
		t.Fatal("expected error when no consolidation run exists")
	}
}

// TestUndoLastConsolidationNilDatabase covers the nil-engine guard.
func TestUndoLastConsolidationNilDatabase(t *testing.T) {
	eng := &Engine{}
	_, err := eng.UndoLastConsolidation(context.Background())
	if err == nil {
		t.Fatal("expected error for nil database")
	}
}

// TestHardDeleteMemoryTxDeletesRow verifies hardDeleteMemoryTx removes the
// row inside the caller's transaction.
func TestHardDeleteMemoryTxDeletesRow(t *testing.T) {
	database, _ := newEngineTestDB(t)
	m := &db.Memory{
		ID: "del-1", Content: "to delete", Scope: "global",
		ConsolidationStatus: "raw", Metadata: map[string]string{},
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("seed: %v", err)
	}

	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatalf("begin tx: %v", err)
	}
	defer tx.Rollback()
	if err := hardDeleteMemoryTx(tx, "del-1"); err != nil {
		t.Fatalf("hardDeleteMemoryTx: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit: %v", err)
	}

	got, err := database.GetMemory("del-1")
	if err != nil || got != nil {
		t.Errorf("expected del-1 to be gone, got %+v (err=%v)", got, err)
	}
}

// TestHardDeleteMemoryTxUnknownIDIsNoop verifies deleting a non-existent row
// is not an error (DELETE semantics).
func TestHardDeleteMemoryTxUnknownIDIsNoop(t *testing.T) {
	database, _ := newEngineTestDB(t)
	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatalf("begin tx: %v", err)
	}
	defer tx.Rollback()
	if err := hardDeleteMemoryTx(tx, "does-not-exist"); err != nil {
		t.Fatalf("expected no error deleting unknown id, got: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit: %v", err)
	}
}

// TestBuildRunSummaryDeduplicates ensures buildRunSummary deduplicates IDs
// across scopes and merges replacement maps.
func TestBuildRunSummaryDeduplicates(t *testing.T) {
	summaries := []ScopeChangeSummary{
		{
			NewMemories:       []*db.Memory{{ID: "n1"}, {ID: "n2"}},
			ArchivedMemoryIDs: []string{"a1"},
			ReplacedIDToNewID: map[string]string{"a1": "n1"},
		},
		{
			NewMemories:       []*db.Memory{{ID: "n1"}}, // duplicate
			ArchivedMemoryIDs: []string{"a1", "a2"},
			ReplacedIDToNewID: map[string]string{"a2": "n2"},
		},
	}
	rs := buildRunSummary(summaries)
	if len(rs.NewMemoryIDs) != 2 {
		t.Errorf("expected 2 deduplicated new memory ids, got %v", rs.NewMemoryIDs)
	}
	if len(rs.ArchivedIDs) != 2 {
		t.Errorf("expected 2 deduplicated archived ids, got %v", rs.ArchivedIDs)
	}
	if rs.ReplacedToNew["a2"] != "n2" {
		t.Errorf("expected replaced map to merge a2→n2, got %v", rs.ReplacedToNew)
	}
}

var _ = sql.ErrNoRows // keep database/sql import for future tx tests
var _ = os.Getenv     // keep os import if helpers evolve
