package consolidation

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/google/uuid"
)

// buildRunSummary assembles a RunSummary from the per-scope summaries so the
// undo operation knows which memories to delete, which to re-activate, and how
// to reparent evidence.
func buildRunSummary(summaries []ScopeChangeSummary) *db.RunSummary {
	rs := &db.RunSummary{
		NewMemoryIDs:  make([]string, 0),
		ArchivedIDs:   make([]string, 0),
		ReplacedToNew: make(map[string]string),
	}
	seenNew := make(map[string]bool)
	seenArch := make(map[string]bool)
	for _, s := range summaries {
		for _, m := range s.NewMemories {
			if !seenNew[m.ID] {
				rs.NewMemoryIDs = append(rs.NewMemoryIDs, m.ID)
				seenNew[m.ID] = true
			}
		}
		for _, id := range s.ArchivedMemoryIDs {
			if !seenArch[id] {
				rs.ArchivedIDs = append(rs.ArchivedIDs, id)
				seenArch[id] = true
			}
		}
		for k, v := range s.ReplacedIDToNewID {
			rs.ReplacedToNew[k] = v
		}
	}
	return rs
}

// journalRun persists a consolidation run to the database journal so it can be
// undone later. It is a no-op when dryRun is true or summaries is empty.
func (eng *Engine) journalRun(dryRun bool, summaries []ScopeChangeSummary) {
	if dryRun || len(summaries) == 0 {
		return
	}
	if eng.database == nil {
		return
	}
	rs := buildRunSummary(summaries)
	summaryJSON, err := db.MarshalRunSummary(rs)
	if err != nil {
		fmt.Fprintf(os.Stderr, "consolidation: failed to marshal run summary for journal: %v\n", err)
		return
	}
	totalArchived := len(rs.ArchivedIDs)
	totalConsolidated := len(rs.NewMemoryIDs)

	run := &db.ConsolidationRun{
		ID:                uuid.New().String(),
		RunAt:             time.Now().UTC(),
		Status:            "completed",
		SummaryJSON:       summaryJSON,
		TotalArchived:     totalArchived,
		TotalConsolidated: totalConsolidated,
	}
	if err := eng.database.SaveConsolidationRun(run); err != nil {
		fmt.Fprintf(os.Stderr, "consolidation: failed to save run journal: %v\n", err)
	}
}

// UndoLastConsolidation reverses the last completed consolidation run. It:
//  1. Hard-deletes all consolidated memories created by that run.
//  2. Re-activates archived originals (status → 'raw', consolidated_into_id → NULL).
//  3. Reparents evidence from deleted consolidated memories back to the originals.
//  4. Marks the run as 'undone' in the journal.
//
// Returns a human-readable summary of what was undone, or an error.
func (eng *Engine) UndoLastConsolidation(ctx context.Context) (string, error) {
	if eng.database == nil {
		return "", fmt.Errorf("consolidation engine has no database connection")
	}

	run, err := eng.database.GetLastCompletedRun()
	if err != nil {
		return "", fmt.Errorf("failed to query last consolidation run: %w", err)
	}
	if run == nil {
		return "", fmt.Errorf("no completed consolidation run found to undo")
	}

	rs, err := db.ParseRunSummary(run.SummaryJSON)
	if err != nil {
		return "", fmt.Errorf("failed to parse run summary: %w", err)
	}

	tx, err := eng.database.BeginTransaction()
	if err != nil {
		return "", fmt.Errorf("failed to begin transaction: %w", err)
	}
	defer tx.Rollback()

	// Step 1: Delete all consolidated memories created by the run.
	for _, id := range rs.NewMemoryIDs {
		if err := hardDeleteMemoryTx(tx, id); err != nil {
			return "", fmt.Errorf("failed to delete consolidated memory %s: %w", id, err)
		}
	}

	// Step 2: Move evidence from deleted consolidated memories back to their
	// originals. Iterate ReplacedToNew in reverse: for each (oldID → newID),
	// move evidence from newID back to oldID.
	// When multiple oldIDs map to the same newID (merged memory), the last
	// oldID in the iteration receives all evidence — this is the best we can
	// do without per-evidence-span tracking.
	for oldID, newID := range rs.ReplacedToNew {
		if oldID == newID {
			continue // single-memory case, no evidence to reparent
		}
		if err := eng.database.ReparentMemoryEvidenceTx(tx, newID, oldID); err != nil {
			return "", fmt.Errorf("failed to reparent evidence from %s to %s: %w", newID, oldID, err)
		}
	}

	// Step 3: Re-activate archived memories (set status back to 'raw' and
	// clear consolidated_into_id).
	for _, id := range rs.ArchivedIDs {
		if err := eng.database.UpdateMemoryStatusTx(tx, id, "raw", ""); err != nil {
			return "", fmt.Errorf("failed to re-activate memory %s: %w", id, err)
		}
	}

	// Step 4: Mark the journal entry as undone — inside the transaction so
	// the single-connection SQLite driver does not deadlock.
	if err := eng.database.MarkRunUndoneTx(tx, run.ID); err != nil {
		return "", fmt.Errorf("failed to mark run %s as undone: %w", run.ID, err)
	}

	if err := tx.Commit(); err != nil {
		return "", fmt.Errorf("failed to commit undo transaction: %w", err)
	}

	summary := fmt.Sprintf(
		"Undid consolidation run %s (from %s):\n"+
			"  Deleted consolidated memories: %d\n"+
			"  Re-activated archived originals: %d\n"+
			"  Reparented evidence pairs: %d",
		run.ID[:8], run.RunAt.Format("2006-01-02 15:04:05"),
		len(rs.NewMemoryIDs),
		len(rs.ArchivedIDs),
		len(rs.ReplacedToNew),
	)
	return summary, nil
}

// hardDeleteMemoryTx deletes a memory row by ID without loading it first.
func hardDeleteMemoryTx(tx *sql.Tx, id string) error {
	_, err := tx.Exec("DELETE FROM memories WHERE id = ?", id)
	return err
}
