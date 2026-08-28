package db

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// memoryCanonicalColumns is the single source of truth for the memories table
// row mapping: 33 columns in the exact order used by INSERT statements.
// The full-row (30-col) and lite (25-col) projections derive from this list
// by omitting computed or embedding-heavy columns.
// Do not reorder, rename, add, or remove entries without updating the
// scanners, INSERT, and UPDATE statements.
var memoryCanonicalColumns = []string{
	"id", "content", "scope", "metadata", "embedding", "embedding_binary",
	"embedding_dim", "embedding_source", "embedding_model", "embedding_quantization",
	"content_hash", "lsh_hash", "created_at", "updated_at", "created_by", "updated_by",
	"created_session", "updated_session", "consolidation_status", "consolidated_into_id",
	"importance", "valid_from", "valid_to", "superseded_by", "tier", "expires_at",
	"access_count", "last_access", "prev_access", "review_status", "kind",
	"decay_factor", "retired_at",
}

// memoryFullOmitted are columns present in the table but omitted from the
// 30-column full scan because they can be recomputed (embedding_dim,
// content_hash, lsh_hash) or are only needed on write.
var memoryFullOmitted = map[string]struct{}{
	"embedding_dim": {},
	"content_hash":  {},
	"lsh_hash":      {},
}

// memoryLiteOmitted are the embedding-related columns excluded from the
// 25-column lite projection (superset of memoryFullOmitted).
var memoryLiteOmitted = map[string]struct{}{
	"embedding":              {},
	"embedding_binary":       {},
	"embedding_dim":          {},
	"embedding_source":       {},
	"embedding_model":        {},
	"embedding_quantization": {},
	"content_hash":           {},
	"lsh_hash":               {},
}

// memoryColumns is the 30-column SELECT list used by scanMemory and
// other full-row queries. Derived from memoryCanonicalColumns minus
// memoryFullOmitted.
var memoryColumns = func() string {
	var parts []string
	for _, c := range memoryCanonicalColumns {
		if _, omit := memoryFullOmitted[c]; !omit {
			parts = append(parts, c)
		}
	}
	return strings.Join(parts, ", ")
}()

// memoryColumnsLite is the 25-column SELECT list used by scanMemoryLite
// and other embedding-omitted queries. Derived from memoryCanonicalColumns
// minus memoryLiteOmitted.
var memoryColumnsLite = func() string {
	var parts []string
	for _, c := range memoryCanonicalColumns {
		if _, omit := memoryLiteOmitted[c]; !omit {
			parts = append(parts, c)
		}
	}
	return strings.Join(parts, ", ")
}()

// MemoryInsertArgs returns the bind arguments for a full INSERT into the
// memories table, in the exact order of memoryCanonicalColumns. This is
// the single source of truth for the 33-value positional list that was
// previously inlined in saveMemoryExec.
func MemoryInsertArgs(m *Memory, now time.Time, quantizeBinary bool) ([]interface{}, error) {
	metadataJSON, err := json.Marshal(m.Metadata)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal metadata: %w", err)
	}

	embeddingJSON, err := json.Marshal(m.Embedding)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal embedding: %w", err)
	}

	embeddingDim := len(m.Embedding)
	lshHash, err := ComputeLSH(m.Embedding)
	if err != nil {
		return nil, err
	}
	var embBin []byte
	if quantizeBinary && len(m.Embedding) > 0 {
		embBin = BinarizeVector(m.Embedding)
	}

	contentHash := m.ContentHash
	if contentHash == "" {
		contentHash = ComputeContentHash(m.Content)
	}

	status := m.ConsolidationStatus
	if status == "" {
		status = "raw"
	}
	reviewStatus := m.ReviewStatus
	if reviewStatus == "" {
		reviewStatus = ReviewApproved
	}
	decayFactor := m.DecayFactor
	if decayFactor <= 0 || decayFactor > 1 {
		decayFactor = 1.0
	}
	var retiredAt sql.NullTime
	if m.RetiredAt != nil {
		retiredAt.Time = m.RetiredAt.UTC()
		retiredAt.Valid = true
	}
	var consolidatedInto sql.NullString
	if m.ConsolidatedIntoID != "" {
		consolidatedInto.String = m.ConsolidatedIntoID
		consolidatedInto.Valid = true
	}
	var validFrom, validTo sql.NullTime
	if m.ValidFrom != nil {
		validFrom.Time = *m.ValidFrom
		validFrom.Valid = true
	} else {
		validFrom.Time = now
		validFrom.Valid = true
	}
	if m.ValidTo != nil {
		validTo.Time = *m.ValidTo
		validTo.Valid = true
	}
	var supersededBy sql.NullString
	if m.SupersededBy != "" {
		supersededBy.String = m.SupersededBy
		supersededBy.Valid = true
	}
	tier := m.Tier
	if tier == "" {
		tier = "long_term"
	}
	accessCount := m.AccessCount
	if accessCount == 0 {
		accessCount = 1
	}
	var expiresAt sql.NullTime
	if m.ExpiresAt != nil {
		expiresAt.Time = m.ExpiresAt.UTC()
		expiresAt.Valid = true
	}
	var lastAccess sql.NullTime
	if m.LastAccess != nil {
		lastAccess.Time = *m.LastAccess
		lastAccess.Valid = true
	}
	var prevAccess sql.NullTime
	if m.PrevAccess != nil {
		prevAccess.Time = *m.PrevAccess
		prevAccess.Valid = true
	}

	return []interface{}{
		m.ID, m.Content, m.Scope, string(metadataJSON), string(embeddingJSON),
		embBin, embeddingDim, m.EmbeddingSource, m.EmbeddingModel, m.EmbeddingQuantization,
		contentHash, lshHash, m.CreatedAt, m.UpdatedAt, m.CreatedBy, m.UpdatedBy,
		m.CreatedSession, m.UpdatedSession, status, consolidatedInto, m.Importance,
		validFrom, validTo, supersededBy, tier, expiresAt, accessCount, lastAccess,
		prevAccess, reviewStatus, m.Kind, decayFactor, retiredAt,
	}, nil
}
