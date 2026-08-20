package db

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-corekit/evidencekit"
)

func mustSaveMemory(t *testing.T, database *DB, id, content string) *Memory {
	t.Helper()
	m := &Memory{
		ID:                  id,
		Content:             content,
		Scope:               "agent",
		Metadata:            map[string]string{},
		CreatedAt:           time.Now().UTC(),
		ConsolidationStatus: "raw",
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save memory: %v", err)
	}
	return m
}

func TestSaveMemoryEvidence_PersistsGroundedExtractions(t *testing.T) {
	database := newTestDB(t)
	m := mustSaveMemory(t, database, "mem-evidence-1", "User prefers dark mode")

	ext := evidencekit.Extraction{
		Source:          evidencekit.SourceRef{ID: "session-1", Kind: "chat"},
		Text:            "User prefers dark mode",
		EvidenceText:    "I always use dark mode",
		Span:            evidencekit.Span{Start: 0, End: 23},
		AlignmentStatus: evidencekit.AlignmentExact,
	}

	if err := database.SaveMemoryEvidence(m.ID, []evidencekit.Extraction{ext}); err != nil {
		t.Fatalf("SaveMemoryEvidence failed: %v", err)
	}

	spans, err := database.GetMemoryEvidence(m.ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence failed: %v", err)
	}
	if len(spans) != 1 {
		t.Fatalf("expected 1 evidence row, got %d", len(spans))
	}
	got := spans[0]
	if got.MemoryID != m.ID || got.SourceID != "session-1" || got.SourceKind != "chat" ||
		got.EvidenceText != ext.EvidenceText || got.CharStart != 0 || got.CharEnd != 23 ||
		got.AlignmentStatus != string(evidencekit.AlignmentExact) {
		t.Errorf("unexpected evidence row: %+v", got)
	}
}

func TestSaveMemoryEvidence_SkipsUnmatched(t *testing.T) {
	database := newTestDB(t)
	m := mustSaveMemory(t, database, "mem-evidence-2", "User prefers dark mode")

	ext := evidencekit.Extraction{
		EvidenceText:    "this was never in any source",
		AlignmentStatus: evidencekit.AlignmentUnmatched,
	}

	if err := database.SaveMemoryEvidence(m.ID, []evidencekit.Extraction{ext}); err != nil {
		t.Fatalf("SaveMemoryEvidence failed: %v", err)
	}

	spans, err := database.GetMemoryEvidence(m.ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence failed: %v", err)
	}
	if len(spans) != 0 {
		t.Errorf("expected unmatched extraction to be skipped, got %d rows", len(spans))
	}
}

func TestMemoryEvidence_CascadeDeletesWithMemory(t *testing.T) {
	database := newTestDB(t)
	m := mustSaveMemory(t, database, "mem-evidence-3", "User prefers dark mode")

	ext := evidencekit.Extraction{
		EvidenceText:    "I always use dark mode",
		Span:            evidencekit.Span{Start: 0, End: 23},
		AlignmentStatus: evidencekit.AlignmentExact,
	}
	if err := database.SaveMemoryEvidence(m.ID, []evidencekit.Extraction{ext}); err != nil {
		t.Fatalf("SaveMemoryEvidence failed: %v", err)
	}

	if err := database.DeleteMemory(m.ID); err != nil {
		t.Fatalf("DeleteMemory failed: %v", err)
	}

	spans, err := database.GetMemoryEvidence(m.ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence failed: %v", err)
	}
	if len(spans) != 0 {
		t.Errorf("expected evidence rows to cascade-delete with memory, got %d", len(spans))
	}
}

func TestReparentMemoryEvidenceTx_MovesRows(t *testing.T) {
	database := newTestDB(t)
	oldMem := mustSaveMemory(t, database, "mem-evidence-old", "raw fact")
	newMem := mustSaveMemory(t, database, "mem-evidence-new", "consolidated fact")

	ext := evidencekit.Extraction{
		EvidenceText:    "raw fact",
		Span:            evidencekit.Span{Start: 0, End: 8},
		AlignmentStatus: evidencekit.AlignmentExact,
	}
	if err := database.SaveMemoryEvidence(oldMem.ID, []evidencekit.Extraction{ext}); err != nil {
		t.Fatalf("SaveMemoryEvidence failed: %v", err)
	}

	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatalf("BeginTransaction failed: %v", err)
	}
	if err := database.ReparentMemoryEvidenceTx(tx, oldMem.ID, newMem.ID); err != nil {
		_ = tx.Rollback()
		t.Fatalf("ReparentMemoryEvidenceTx failed: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit failed: %v", err)
	}

	oldSpans, err := database.GetMemoryEvidence(oldMem.ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence(old) failed: %v", err)
	}
	if len(oldSpans) != 0 {
		t.Errorf("expected 0 evidence rows left on old memory, got %d", len(oldSpans))
	}

	newSpans, err := database.GetMemoryEvidence(newMem.ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence(new) failed: %v", err)
	}
	if len(newSpans) != 1 {
		t.Fatalf("expected 1 evidence row on new memory, got %d", len(newSpans))
	}
	if newSpans[0].MemoryID != newMem.ID {
		t.Errorf("expected reparented row memory_id = %s, got %s", newMem.ID, newSpans[0].MemoryID)
	}
}
