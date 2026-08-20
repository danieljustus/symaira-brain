package db

import (
	"database/sql"
	"time"

	"github.com/danieljustus/symaira-corekit/evidencekit"
	"github.com/google/uuid"
)

// EvidenceSpan is a persisted evidencekit.Extraction linked to the memory it
// was extracted for.
type EvidenceSpan struct {
	ID              string    `json:"id"`
	MemoryID        string    `json:"memory_id"`
	SourceID        string    `json:"source_id,omitempty"`
	SourceKind      string    `json:"source_kind,omitempty"`
	Text            string    `json:"text,omitempty"`
	EvidenceText    string    `json:"evidence_text"`
	CharStart       int       `json:"char_start"`
	CharEnd         int       `json:"char_end"`
	AlignmentStatus string    `json:"alignment_status"`
	CreatedAt       time.Time `json:"created_at"`
}

// saveMemoryEvidenceExec is the shared implementation for SaveMemoryEvidence
// and SaveMemoryEvidenceTx. Extractions that fail evidencekit.Validate
// (unmatched alignment, invalid span, empty evidence text) are skipped —
// only grounded evidence is persisted.
func saveMemoryEvidenceExec(execer SQLExecer, memoryID string, extractions []evidencekit.Extraction) error {
	now := time.Now().UTC()
	for _, ext := range extractions {
		if err := evidencekit.Validate(ext); err != nil {
			continue
		}
		_, err := execer.Exec(
			`INSERT INTO memory_evidence (id, memory_id, source_id, source_kind, text, evidence_text, char_start, char_end, alignment_status, created_at)
			 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			uuid.New().String(), memoryID, ext.Source.ID, ext.Source.Kind, ext.Text, ext.EvidenceText, ext.Span.Start, ext.Span.End, string(ext.AlignmentStatus), now,
		)
		if err != nil {
			return err
		}
	}
	return nil
}

// SaveMemoryEvidence persists grounded extractions for a memory.
func (db *DB) SaveMemoryEvidence(memoryID string, extractions []evidencekit.Extraction) error {
	return saveMemoryEvidenceExec(db.conn, memoryID, extractions)
}

// SaveMemoryEvidenceTx persists grounded extractions for a memory within a transaction.
func (db *DB) SaveMemoryEvidenceTx(tx *sql.Tx, memoryID string, extractions []evidencekit.Extraction) error {
	return saveMemoryEvidenceExec(tx, memoryID, extractions)
}

// GetMemoryEvidence returns all evidence spans stored for a memory, oldest first.
func (db *DB) GetMemoryEvidence(memoryID string) ([]EvidenceSpan, error) {
	rows, err := db.conn.Query(
		`SELECT id, memory_id, source_id, source_kind, text, evidence_text, char_start, char_end, alignment_status, created_at
		 FROM memory_evidence WHERE memory_id = ? ORDER BY created_at ASC`,
		memoryID,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var spans []EvidenceSpan
	for rows.Next() {
		var s EvidenceSpan
		if err := rows.Scan(&s.ID, &s.MemoryID, &s.SourceID, &s.SourceKind, &s.Text, &s.EvidenceText, &s.CharStart, &s.CharEnd, &s.AlignmentStatus, &s.CreatedAt); err != nil {
			return nil, err
		}
		spans = append(spans, s)
	}
	return spans, nil
}

// ReparentMemoryEvidenceTx re-links evidence rows from an old memory ID to a
// new one within a transaction. Used by consolidation when raw memories are
// replaced by a newly synthesized consolidated memory.
func (db *DB) ReparentMemoryEvidenceTx(tx *sql.Tx, oldMemoryID, newMemoryID string) error {
	if oldMemoryID == "" || newMemoryID == "" || oldMemoryID == newMemoryID {
		return nil
	}
	_, err := tx.Exec(`UPDATE memory_evidence SET memory_id = ? WHERE memory_id = ?`, newMemoryID, oldMemoryID)
	return err
}
