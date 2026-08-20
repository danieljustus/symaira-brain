package db

import (
	"crypto/sha256"
	"database/sql"
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// Memory represents a single saved fact or context snippet.
type Memory struct {
	ID                    string            `json:"id"`
	Content               string            `json:"content"`
	Scope                 string            `json:"scope"`               // global, project, agent, user, session
	Metadata              map[string]string `json:"metadata"`            // key-value metadata
	Embedding             []float32         `json:"embedding,omitempty"` // semantic embedding
	EmbeddingBinary       []byte            `json:"-"`                   // 96-byte sign-bit binary vector for Hamming prefilter
	EmbeddingSource       string            `json:"embedding_source,omitempty"`
	EmbeddingModel        string            `json:"embedding_model,omitempty"`
	EmbeddingQuantization string            `json:"embedding_quantization,omitempty"` // quantization level ("" = unquantized)
	ContentHash           string            `json:"content_hash,omitempty"`
	CreatedAt             time.Time         `json:"created_at"`
	UpdatedAt             time.Time         `json:"updated_at"`
	CreatedBy             string            `json:"created_by,omitempty"`
	UpdatedBy             string            `json:"updated_by,omitempty"`
	CreatedSession        string            `json:"created_session,omitempty"`
	UpdatedSession        string            `json:"updated_session,omitempty"`
	Entities              []string          `json:"entities,omitempty"` // linked entity names
	ConsolidationStatus   string            `json:"consolidation_status"`
	ConsolidatedIntoID    string            `json:"consolidated_into_id,omitempty"`
	Importance            float64           `json:"importance"` // 0.0–1.0, default 0.5
	ValidFrom             *time.Time        `json:"valid_from,omitempty"`
	ValidTo               *time.Time        `json:"valid_to,omitempty"`
	SupersededBy          string            `json:"superseded_by,omitempty"`
	Kind                  string            `json:"kind,omitempty"`         // semantic kind: user, feedback, project, reference ("" = unclassified, #486)
	ReviewStatus          string            `json:"review_status"`          // "approved" (live) or "staged" (candidate, #485)
	DecayFactor           float64           `json:"decay_factor,omitempty"` // aging multiplier in (0,1], default 1.0 (#491)
	RetiredAt             *time.Time        `json:"retired_at,omitempty"`   // when set, memory is retired (flagged, never hard-deleted, #491)
	Tier                  string            `json:"tier"`                   // "long_term" (default) or "working"
	ExpiresAt             *time.Time        `json:"expires_at,omitempty"`   // when tier=working, evict after this time
	AccessCount           int64             `json:"access_count"`           // number of times retrieved (feedback loop)
	LastAccess            *time.Time        `json:"last_access,omitempty"`  // last retrieval timestamp
	PrevAccess            *time.Time        `json:"prev_access,omitempty"`  // last_access before the most recent retrieval (#489, spacing-aware reinforcement)
	Evidence              []EvidenceSpan    `json:"evidence,omitempty"`     // populated only on demand (e.g. --with-evidence), not by GetMemory/scanMemory
}

// SearchResult wraps a Memory with its similarity score without mutating the original.
// SourceProfile and SourceScope carry provenance when a context profile was used.
type SearchResult struct {
	Memory        *Memory `json:"memory"`
	Score         float32 `json:"similarity_score"`
	SourceProfile string  `json:"source_profile,omitempty"` // non-empty when a context profile produced this result
	SourceScope   string  `json:"source_scope,omitempty"`   // the effective scope this result came from
}

// RankingWeights holds configurable weights for composite retrieval scoring.
type RankingWeights struct {
	RelevanceWeight           float64
	RecencyWeight             float64
	ImportanceWeight          float64
	AccessReinforcementWeight float64 // boost for frequently accessed memories (0 = disabled)
	RecencyHalfLife           float64 // days
	AccessHalfLife            float64 // days for last-access recency decay
	AccessSpacingHalfLife     float64 // days for the spacing-aware reinforcement gap (#489)
	SpreadingWeight           float64 // weight of the memory-association bonus (#488, 0 = disabled)
}

// TrustFilter defines retrieval filters for trust-aware memory search.
// Empty fields are ignored (no filtering on that dimension).
type TrustFilter struct {
	MinConfidence      string        // "low", "medium", "high" — skip memories below this
	VerificationStatus string        // "verified", "unverified", "stale" — filter by verification
	ExcludeSuperseded  bool          // when true, skip memories with non-empty superseded_by
	MaxAge             time.Duration // when non-zero, skip memories older than this
}

// PolicyFilter defines sensitivity and sharing policy filters for memory retrieval.
// Empty fields are ignored (no filtering on that dimension).
type PolicyFilter struct {
	MaxSensitivity  string // "public", "internal", "confidential", "secret" — skip memories above this
	MinSharingLevel string // "private", "team", "org", "public" — skip memories below this
	ClientID        string // when non-empty, check against allowed_clients metadata
}

// TimeWindow defines an optional valid_from/valid_to filter for memory retrieval.
// Both fields are optional; when nil the constraint on that side is relaxed.
// Semantics: memory.valid_from <= To AND (memory.valid_to IS NULL OR memory.valid_to >= From).
// A NULL valid_from is treated as unbounded in the past, a NULL valid_to as unbounded in the future.
type TimeWindow struct {
	From *time.Time // memories must be valid at or after this time (valid_to >= From or valid_to IS NULL)
	To   *time.Time // memories must be valid at or before this time (valid_from <= To or valid_from IS NULL)
}

// TimeWindowClause returns SQL WHERE clause fragments and args for the time window.
// Returns (whereClause, args) where whereClause includes the leading " AND ".
func TimeWindowClause(tw TimeWindow, tableAlias string) (string, []interface{}) {
	if tw.From == nil && tw.To == nil {
		return "", nil
	}
	prefix := tableAlias
	if prefix != "" {
		prefix += "."
	}
	var clauses []string
	var args []interface{}
	if tw.From != nil {
		clauses = append(clauses, "("+prefix+"valid_to IS NULL OR "+prefix+"valid_to >= ?)")
		args = append(args, *tw.From)
	}
	if tw.To != nil {
		clauses = append(clauses, "("+prefix+"valid_from IS NULL OR "+prefix+"valid_from <= ?)")
		args = append(args, *tw.To)
	}
	return " AND " + strings.Join(clauses, " AND "), args
}

// passesTimeWindow checks whether a single memory falls within the given time window.
func passesTimeWindow(m *Memory, tw TimeWindow) bool {
	if tw.From != nil {
		if m.ValidTo != nil && m.ValidTo.Before(*tw.From) {
			return false
		}
	}
	if tw.To != nil {
		if m.ValidFrom != nil && m.ValidFrom.After(*tw.To) {
			return false
		}
	}
	return true
}

// ComputeContentHash returns the SHA-256 hex digest of the given content string.
func ComputeContentHash(content string) string {
	h := sha256.Sum256([]byte(content))
	return fmt.Sprintf("%x", h)
}

// saveMemoryExec is the shared implementation for SaveMemory and SaveMemoryTx.
func saveMemoryExec(execer SQLExecer, m *Memory, quantizeBinary bool) error {
	metadataJSON, err := json.Marshal(m.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %w", err)
	}

	embeddingJSON, err := json.Marshal(m.Embedding)
	if err != nil {
		return fmt.Errorf("failed to marshal embedding: %w", err)
	}

	embeddingDim := len(m.Embedding)
	lshHash, err := ComputeLSH(m.Embedding)
	if err != nil {
		return err
	}
	var embBin []byte
	if quantizeBinary && len(m.Embedding) > 0 {
		embBin = BinarizeVector(m.Embedding)
	}

	// Compute content hash if not already set.
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
		now := time.Now().UTC()
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
		// Normalize to UTC: the persistence layer stores expires_at as
		// text and the working-memory queries compare it lexicographically
		// against datetime('now') (UTC). Storing local time (+0200 etc.)
		// made expired working memories compare as not-yet-expired.
		expiresAt.Time = m.ExpiresAt.UTC()
		expiresAt.Valid = true
	}

	query := `INSERT INTO memories (id, content, scope, metadata, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			content=excluded.content,
			scope=excluded.scope,
			metadata=excluded.metadata,
			embedding=excluded.embedding,
			embedding_binary=excluded.embedding_binary,
			embedding_dim=excluded.embedding_dim,
			embedding_source=excluded.embedding_source,
			embedding_model=excluded.embedding_model,
			embedding_quantization=excluded.embedding_quantization,
			content_hash=excluded.content_hash,
			lsh_hash=excluded.lsh_hash,
			updated_at=excluded.updated_at,
			updated_by=excluded.updated_by,
			updated_session=excluded.updated_session,
			consolidation_status=excluded.consolidation_status,
			consolidated_into_id=excluded.consolidated_into_id,
			importance=excluded.importance,
			valid_from=excluded.valid_from,
			valid_to=excluded.valid_to,
			superseded_by=excluded.superseded_by,
			tier=excluded.tier,
			expires_at=excluded.expires_at,
			access_count=excluded.access_count,
			review_status=excluded.review_status,
			kind=excluded.kind,
			decay_factor=excluded.decay_factor,
			retired_at=excluded.retired_at`

	now := time.Now().UTC()
	if m.CreatedAt.IsZero() {
		m.CreatedAt = now
	}
	m.UpdatedAt = now

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

	_, err = execer.Exec(query, m.ID, m.Content, m.Scope, string(metadataJSON), string(embeddingJSON), embBin, embeddingDim, m.EmbeddingSource, m.EmbeddingModel, m.EmbeddingQuantization, contentHash, lshHash, m.CreatedAt, m.UpdatedAt, m.CreatedBy, m.UpdatedBy, m.CreatedSession, m.UpdatedSession, status, consolidatedInto, m.Importance, validFrom, validTo, supersededBy, tier, expiresAt, accessCount, lastAccess, prevAccess, reviewStatus, m.Kind, decayFactor, retiredAt)
	return err
}

// SaveMemory inserts or updates a memory. Every successful write records a
// "set" audit event so mutations leave a trace regardless of the entry point
// (CLI, MCP, HTTP, importers).
func (db *DB) SaveMemory(m *Memory) error {
	if err := saveMemoryExec(db.conn, m, db.quantizeBinary); err != nil {
		return err
	}
	_ = db.LogAudit("set", m.ID, m.Scope, m.CreatedSession, m.CreatedBy, "")
	return nil
}

// SaveMemoryTx inserts or updates a memory within a transaction.
func (db *DB) SaveMemoryTx(tx *sql.Tx, m *Memory) error {
	return saveMemoryExec(tx, m, db.quantizeBinary)
}

// UpdateMemoryStatusTx updates the consolidation status and parent ID of a memory within a transaction.
func (db *DB) UpdateMemoryStatusTx(tx *sql.Tx, id string, status string, parentID string) error {
	var consolidatedInto sql.NullString
	if parentID != "" {
		consolidatedInto.String = parentID
		consolidatedInto.Valid = true
	}
	query := `UPDATE memories SET consolidation_status = ?, consolidated_into_id = ?, updated_at = ? WHERE id = ?`
	_, err := tx.Exec(query, status, consolidatedInto, time.Now().UTC(), id)
	return err
}

// DeleteMemory removes a memory by ID. A "delete" audit event is recorded
// with the memory's scope, session and recorded creator as the best identity
// available at the storage layer.
func (db *DB) DeleteMemory(id string) error {
	var scope, createdBy, createdSession sql.NullString
	err := db.conn.QueryRow("SELECT scope, created_by, created_session FROM memories WHERE id = ?", id).Scan(&scope, &createdBy, &createdSession)
	if err == sql.ErrNoRows {
		return nil
	}
	if err != nil {
		return err
	}
	if _, err := db.conn.Exec("DELETE FROM memories WHERE id = ?", id); err != nil {
		return err
	}
	_ = db.LogAudit("delete", id, scope.String, createdSession.String, createdBy.String, "")
	return nil
}

// GetMemory retrieves a single memory by its ID using a direct index lookup.
// It also tracks the access for retrieval feedback.
func (db *DB) GetMemory(id string) (*Memory, error) {
	m, err := db.loadMemory(id)
	if err != nil || m == nil {
		return m, err
	}
	// Track access for retrieval feedback loop.
	if err := db.TrackMemoryAccess(m.ID); err != nil {
		return nil, err
	}
	return m, nil
}

// loadMemory hydrates a memory row by ID without side effects (no access
// tracking). Used by internal lookups such as the write-path duplicate
// check, where a lookup must not count as a retrieval.
func (db *DB) loadMemory(id string) (*Memory, error) {
	var m Memory
	var metaStr, embStr string
	var embBin []byte
	var consolidatedInto sql.NullString
	var validFrom, validTo sql.NullTime
	var supersededBy sql.NullString
	var expiresAt sql.NullTime
	var lastAccess sql.NullTime
	var reviewStatus, kind string
	var decayFactor float64
	var retiredAt sql.NullTime
	var prevAccess sql.NullTime
	err := db.conn.QueryRow(
		"SELECT "+memoryColumns+" FROM memories WHERE id = ?",
		id,
	).Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &embStr, &embBin, &m.EmbeddingSource, &m.EmbeddingModel, &m.EmbeddingQuantization, &m.CreatedAt, &m.UpdatedAt, &m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession, &m.ConsolidationStatus, &consolidatedInto, &m.Importance, &validFrom, &validTo, &supersededBy, &m.Tier, &expiresAt, &m.AccessCount, &lastAccess, &prevAccess, &reviewStatus, &kind, &decayFactor, &retiredAt)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	m.ReviewStatus = reviewStatus
	m.Kind = kind
	m.DecayFactor = decayFactor
	if retiredAt.Valid {
		m.RetiredAt = &retiredAt.Time
	}
	if prevAccess.Valid {
		m.PrevAccess = &prevAccess.Time
	}
	if lastAccess.Valid {
		m.LastAccess = &lastAccess.Time
	}
	if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
		return nil, err
	}
	if expiresAt.Valid {
		m.ExpiresAt = &expiresAt.Time
	}
	if err := json.Unmarshal([]byte(embStr), &m.Embedding); err != nil {
		return nil, err
	}
	m.EmbeddingBinary = embBin

	entities, err := db.EntitiesForMemory(m.ID)
	if err == nil && len(entities) > 0 {
		for _, e := range entities {
			m.Entities = append(m.Entities, e.Name)
		}
	}

	return &m, nil
}

func populateMemoryFields(m *Memory, metaStr string, consolidatedInto sql.NullString, validFrom, validTo sql.NullTime, supersededBy sql.NullString) error {
	if err := json.Unmarshal([]byte(metaStr), &m.Metadata); err != nil {
		return err
	}
	m.ConsolidatedIntoID = consolidatedInto.String
	if validFrom.Valid {
		m.ValidFrom = &validFrom.Time
	}
	if validTo.Valid {
		m.ValidTo = &validTo.Time
	}
	m.SupersededBy = supersededBy.String
	return nil
}

// memoryColumns is the full 30-column SELECT list used by scanMemory.
// Any change must be mirrored in scanMemory.
const memoryColumns = "id, content, scope, metadata, embedding, embedding_binary, embedding_source, embedding_model, embedding_quantization, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at"

// memoryColumnsLite is the 25-column SELECT list used by scanMemoryLite
// (omits embedding, embedding_binary, embedding_source, embedding_model, embedding_quantization).
// Any change must be mirrored in scanMemoryLite.
const memoryColumnsLite = "id, content, scope, metadata, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at"

// scanMemory scans a full Memory row (including embedding) from *sql.Rows.
func scanMemory(rows *sql.Rows) (*Memory, error) {
	var m Memory
	var metaStr, embStr string
	var embBin []byte
	var consolidatedInto sql.NullString
	var validFrom, validTo sql.NullTime
	var supersededBy sql.NullString
	var expiresAt sql.NullTime
	var lastAccess sql.NullTime
	var reviewStatus, kind string
	var decayFactor float64
	var retiredAt sql.NullTime
	var prevAccess sql.NullTime
	if err := rows.Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &embStr, &embBin, &m.EmbeddingSource, &m.EmbeddingModel, &m.EmbeddingQuantization, &m.CreatedAt, &m.UpdatedAt, &m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession, &m.ConsolidationStatus, &consolidatedInto, &m.Importance, &validFrom, &validTo, &supersededBy, &m.Tier, &expiresAt, &m.AccessCount, &lastAccess, &prevAccess, &reviewStatus, &kind, &decayFactor, &retiredAt); err != nil {
		return nil, err
	}
	m.ReviewStatus = reviewStatus
	m.Kind = kind
	m.DecayFactor = decayFactor
	if retiredAt.Valid {
		m.RetiredAt = &retiredAt.Time
	}
	if prevAccess.Valid {
		m.PrevAccess = &prevAccess.Time
	}
	if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
		return nil, err
	}
	if expiresAt.Valid {
		m.ExpiresAt = &expiresAt.Time
	}
	if lastAccess.Valid {
		m.LastAccess = &lastAccess.Time
	}
	if err := json.Unmarshal([]byte(embStr), &m.Embedding); err != nil {
		return nil, err
	}
	m.EmbeddingBinary = embBin
	return &m, nil
}

// scanMemoryLite scans a Memory row without embedding data from *sql.Rows.
func scanMemoryLite(rows *sql.Rows) (*Memory, error) {
	var m Memory
	var metaStr string
	var consolidatedInto sql.NullString
	var validFrom, validTo sql.NullTime
	var supersededBy sql.NullString
	var expiresAt sql.NullTime
	var lastAccess sql.NullTime
	var reviewStatus, kind string
	var decayFactor float64
	var retiredAt sql.NullTime
	var prevAccess sql.NullTime
	if err := rows.Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &m.CreatedAt, &m.UpdatedAt, &m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession, &m.ConsolidationStatus, &consolidatedInto, &m.Importance, &validFrom, &validTo, &supersededBy, &m.Tier, &expiresAt, &m.AccessCount, &lastAccess, &prevAccess, &reviewStatus, &kind, &decayFactor, &retiredAt); err != nil {
		return nil, err
	}
	m.ReviewStatus = reviewStatus
	m.Kind = kind
	m.DecayFactor = decayFactor
	if retiredAt.Valid {
		m.RetiredAt = &retiredAt.Time
	}
	if prevAccess.Valid {
		m.PrevAccess = &prevAccess.Time
	}
	if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
		return nil, err
	}
	if expiresAt.Valid {
		m.ExpiresAt = &expiresAt.Time
	}
	if lastAccess.Valid {
		m.LastAccess = &lastAccess.Time
	}
	return &m, nil
}

// ListMemories returns memories with pagination, optionally filtered by scope.
func (db *DB) ListMemories(scope string, offset, limit int) ([]*Memory, error) {
	var query string
	var rows *sql.Rows
	var err error

	if scope != "" {
		query = "SELECT " + memoryColumns + " FROM memories WHERE scope = ? AND consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, scope, limit, offset)
	} else {
		query = "SELECT " + memoryColumns + " FROM memories WHERE consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, limit, offset)
	}

	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemory(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}

	return memories, nil
}

// ListMemoriesAsOf returns memories whose validity window includes asOf,
// i.e. valid_from <= asOf and (valid_to is unset or valid_to > asOf). This
// makes a memory that has since been superseded visible again for a query
// against a timestamp within its original validity window. A memory with
// no valid_from (never explicitly versioned) is treated as always having
// started, so it participates in every as-of query. Same result shape as
// ListMemories.
func (db *DB) ListMemoriesAsOf(scope string, asOf time.Time, offset, limit int) ([]*Memory, error) {
	var query string
	var rows *sql.Rows
	var err error

	const asOfClause = " AND (valid_from IS NULL OR valid_from <= ?) AND (valid_to IS NULL OR valid_to > ?)"
	const expiredWorkingClause = " AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL"

	if scope != "" {
		query = "SELECT " + memoryColumns + " FROM memories WHERE scope = ? AND consolidation_status != 'archived'" + expiredWorkingClause + asOfClause + " ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, scope, asOf, asOf, limit, offset)
	} else {
		query = "SELECT " + memoryColumns + " FROM memories WHERE consolidation_status != 'archived'" + expiredWorkingClause + asOfClause + " ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, asOf, asOf, limit, offset)
	}

	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemory(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}

	return memories, nil
}

// ListMemoriesLite returns memories without embedding data, with pagination.
func (db *DB) ListMemoriesLite(scope string, offset, limit int) ([]*Memory, error) {
	var query string
	var rows *sql.Rows
	var err error

	if scope != "" {
		query = "SELECT " + memoryColumnsLite + " FROM memories WHERE scope = ? AND consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, scope, limit, offset)
	} else {
		query = "SELECT " + memoryColumnsLite + " FROM memories WHERE consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at DESC LIMIT ? OFFSET ?"
		rows, err = db.conn.Query(query, limit, offset)
	}

	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}

	return memories, nil
}

// ListMemoriesFiltered returns memories without embedding data, filtered by scope and optionally by entity.
func (db *DB) ListMemoriesFiltered(scope, entityID string, offset, limit int) ([]*Memory, error) {
	if entityID == "" {
		return db.ListMemoriesLite(scope, offset, limit)
	}

	memoryIDs, err := db.MemoryIDsForEntity(entityID)
	if err != nil {
		return nil, err
	}
	if len(memoryIDs) == 0 {
		return nil, nil
	}

	placeholders := make([]string, len(memoryIDs))
	args := make([]interface{}, 0, len(memoryIDs)+2)
	for i, id := range memoryIDs {
		placeholders[i] = "?"
		args = append(args, id)
	}
	inClause := strings.Join(placeholders, ", ")

	var query string
	if scope != "" {
		query = "SELECT " + memoryColumnsLite + " FROM memories WHERE scope = ? AND consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND id IN (" + inClause + ") ORDER BY created_at DESC LIMIT ? OFFSET ?"
		args = append([]interface{}{scope}, args...)
		args = append(args, limit, offset)
	} else {
		query = "SELECT " + memoryColumnsLite + " FROM memories WHERE consolidation_status != 'archived' AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND id IN (" + inClause + ") ORDER BY created_at DESC LIMIT ? OFFSET ?"
		args = append(args, limit, offset)
	}

	rows, err := db.conn.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}

// ListMemoriesFilteredWithPolicy returns memories filtered by scope and policy.
func (db *DB) ListMemoriesFilteredWithPolicy(scope string, offset, limit int, policyFilter PolicyFilter) ([]*Memory, error) {
	memories, err := db.ListMemoriesLite(scope, offset, limit)
	if err != nil {
		return nil, err
	}

	if policyFilter.MaxSensitivity == "" && policyFilter.MinSharingLevel == "" && policyFilter.ClientID == "" {
		return memories, nil
	}

	var filtered []*Memory
	for _, m := range memories {
		if PassesPolicyFilter(m, policyFilter) {
			filtered = append(filtered, m)
		}
	}
	return filtered, nil
}

// GetMemoriesSince returns all memories with updated_at strictly after t.
// Embedding data is omitted (sync payloads do not need vectors).
func (db *DB) GetMemoriesSince(t time.Time) ([]*Memory, error) {
	return db.GetMemoriesSinceCursor(t, 0)
}

// GetMemoriesSinceCursor returns memories updated after since, with cursor-based pagination.
// When includeEmbeddings is true, the full embedding vector is loaded (needed for sync transfer).
func (db *DB) GetMemoriesSinceCursor(since time.Time, limit int, includeEmbeddings ...bool) ([]*Memory, error) {
	if limit <= 0 {
		limit = 50000
	}
	includeEmb := len(includeEmbeddings) > 0 && includeEmbeddings[0]

	var query string
	if includeEmb {
		query = "SELECT " + memoryColumns + " FROM memories WHERE updated_at > ? ORDER BY updated_at ASC LIMIT ?"
	} else {
		query = "SELECT " + memoryColumnsLite + " FROM memories WHERE updated_at > ? ORDER BY updated_at ASC LIMIT ?"
	}

	rows, err := db.conn.Query(query, since, limit)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var memories []*Memory
	for rows.Next() {
		var m *Memory
		if includeEmb {
			m, err = scanMemory(rows)
		} else {
			m, err = scanMemoryLite(rows)
		}
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}

// GetRawMemories returns all memories with consolidation_status = 'raw'.
func (db *DB) GetRawMemories() ([]*Memory, error) {
	query := "SELECT " + memoryColumns + " FROM memories WHERE consolidation_status = 'raw' AND review_status = 'approved' AND retired_at IS NULL ORDER BY created_at ASC"
	rows, err := db.conn.Query(query)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemory(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}

// UpsertMemoryIfNewer inserts or updates a memory only if the incoming
// updated_at is strictly newer than the stored row. Returns true when the
// row was inserted or overwritten, false when it was skipped.
func (db *DB) UpsertMemoryIfNewer(m *Memory) (bool, error) {
	metadataJSON, err := json.Marshal(m.Metadata)
	if err != nil {
		return false, fmt.Errorf("failed to marshal metadata: %w", err)
	}

	embeddingJSON, err := json.Marshal(m.Embedding)
	if err != nil {
		return false, fmt.Errorf("failed to marshal embedding: %w", err)
	}

	embeddingDim := len(m.Embedding)
	lshHash, err := ComputeLSH(m.Embedding)
	if err != nil {
		return false, err
	}
	var embBin []byte
	if db.quantizeBinary && len(m.Embedding) > 0 {
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
	var consolidatedInto sql.NullString
	if m.ConsolidatedIntoID != "" {
		consolidatedInto.String = m.ConsolidatedIntoID
		consolidatedInto.Valid = true
	}

	var existingUpdated time.Time
	err = db.conn.QueryRow("SELECT updated_at FROM memories WHERE id = ?", m.ID).Scan(&existingUpdated)
	if err != nil && err != sql.ErrNoRows {
		return false, err
	}

	if err == sql.ErrNoRows {
		_, err = db.conn.Exec(
			`INSERT INTO memories (id, content, scope, metadata, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at)
			 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			m.ID, m.Content, m.Scope, string(metadataJSON), string(embeddingJSON), embBin, embeddingDim, m.EmbeddingSource, m.EmbeddingModel, m.EmbeddingQuantization, contentHash, lshHash, m.CreatedAt, m.UpdatedAt, m.CreatedBy, m.UpdatedBy, m.CreatedSession, m.UpdatedSession, status, consolidatedInto, m.Importance, m.ValidFrom, m.ValidTo, nullStr(m.SupersededBy), m.Tier, nullTimePtr(m.ExpiresAt), m.AccessCount, nullTimePtr(m.LastAccess), nullTimePtr(m.PrevAccess), reviewStatus, m.Kind, decayFactor, nullTimePtr(m.RetiredAt),
		)
		if err != nil {
			return false, err
		}
		_ = db.LogAudit("set", m.ID, m.Scope, m.CreatedSession, m.CreatedBy, "")
		return true, nil
	}

	if !m.UpdatedAt.After(existingUpdated) {
		return false, nil
	}

	_, err = db.conn.Exec(
		`UPDATE memories SET content=?, scope=?, metadata=?, embedding=?, embedding_binary=?, embedding_dim=?, embedding_source=?, embedding_model=?, embedding_quantization=?, content_hash=?, lsh_hash=?, updated_at=?, updated_by=?, updated_session=?, consolidation_status=?, consolidated_into_id=?, importance=?, tier=?, expires_at=?, access_count=?, last_access=?, prev_access=?, review_status=?, kind=?, decay_factor=?, retired_at=? WHERE id=?`,
		m.Content, m.Scope, string(metadataJSON), string(embeddingJSON), embBin, embeddingDim, m.EmbeddingSource, m.EmbeddingModel, m.EmbeddingQuantization, contentHash, lshHash, m.UpdatedAt, m.UpdatedBy, m.UpdatedSession, status, consolidatedInto, m.Importance, m.Tier, nullTimePtr(m.ExpiresAt), m.AccessCount, nullTimePtr(m.LastAccess), nullTimePtr(m.PrevAccess), reviewStatus, m.Kind, decayFactor, nullTimePtr(m.RetiredAt), m.ID,
	)
	if err != nil {
		return false, err
	}
	_ = db.LogAudit("update", m.ID, m.Scope, m.UpdatedSession, m.UpdatedBy, "")
	return true, nil
}

// DefaultRankingWeights returns the default ranking configuration.
func DefaultRankingWeights() RankingWeights {
	return RankingWeights{
		RelevanceWeight:           0.6,
		RecencyWeight:             0.2,
		ImportanceWeight:          0.2,
		AccessReinforcementWeight: 0.0,
		RecencyHalfLife:           30,
		AccessHalfLife:            14,
		AccessSpacingHalfLife:     30,
		SpreadingWeight:           0.0,
	}
}

// WeightsFromConfig maps the persisted ranking configuration onto
// retrieval weights. Zero-valued fields keep their defaults, so a partial
// config never silently disables a term.
func WeightsFromConfig(cfg config.RankingConfig) RankingWeights {
	w := DefaultRankingWeights()
	if cfg.RelevanceWeight != 0 {
		w.RelevanceWeight = cfg.RelevanceWeight
	}
	if cfg.RecencyWeight != 0 {
		w.RecencyWeight = cfg.RecencyWeight
	}
	if cfg.ImportanceWeight != 0 {
		w.ImportanceWeight = cfg.ImportanceWeight
	}
	if cfg.AccessReinforcementWeight != 0 {
		w.AccessReinforcementWeight = cfg.AccessReinforcementWeight
	}
	if cfg.RecencyHalfLife != 0 {
		w.RecencyHalfLife = cfg.RecencyHalfLife
	}
	if cfg.AccessHalfLife != 0 {
		w.AccessHalfLife = cfg.AccessHalfLife
	}
	if cfg.AccessSpacingHalfLife != 0 {
		w.AccessSpacingHalfLife = cfg.AccessSpacingHalfLife
	}
	if cfg.SpreadingWeight != 0 {
		w.SpreadingWeight = cfg.SpreadingWeight
	}
	return w
}

// ensureAssociationsSeeded auto-derives memory-association edges once per
// process when the table is empty and the spreading term is enabled
// (#488). Seeding is cheap (bounded per source) and idempotent; failures
// degrade to "no edges" rather than failing the search.
func (db *DB) ensureAssociationsSeeded() {
	db.spreadSeedOnce.Do(func() {
		n, err := db.AssociationCount()
		if err != nil || n > 0 {
			return
		}
		_, _ = db.SeedMemoryAssociations("auto")
	})
}

// SearchMemories uses LSH bucket pre-filtering to avoid full table scans,
// then ranks the reduced candidate set by cosine similarity.
func (db *DB) SearchMemories(queryVec []float32, querySource string, scope string, limit int) ([]SearchResult, error) {
	return db.SearchMemoriesFiltered(queryVec, querySource, scope, limit, "", DefaultRankingWeights())
}

// SearchMemoriesFiltered extends SearchMemories with an optional entity filter.
// When entityID is non-empty, only memories linked to that entity are returned.
// Memories with a non-null embedding whose embedding_source matches querySource
// are filtered at the candidate-query level; rows from a different embedding
// space are never hydrated or scored.
func (db *DB) SearchMemoriesFiltered(queryVec []float32, querySource string, scope string, limit int, entityID string, weights ...RankingWeights) ([]SearchResult, error) {
	return db.SearchMemoriesFilteredWithTrust(queryVec, querySource, scope, limit, entityID, TrustFilter{}, PolicyFilter{}, TimeWindow{}, "", weights...)
}

// SearchMemoriesFilteredWithTrust extends SearchMemoriesFiltered with trust-aware
// and policy-aware filtering. Memories that don't match the trust or policy filter
// are excluded from results.
// SearchQuery carries the full parameter set for a filtered vector search.
// SearchMemoriesFilteredWithTrust builds one and delegates to the named
// stage methods below.
type SearchQuery struct {
	QueryVec     []float32
	QuerySource  string
	Scope        string
	Scopes       []string // when non-empty, candidate selection filters scope IN (Scopes)
	Limit        int
	EntityID     string
	TrustFilter  TrustFilter
	PolicyFilter PolicyFilter
	TimeWindow   TimeWindow
	Quantization string
	Weights      RankingWeights
	// NoAccessTracking, when true, skips the best-effort access-tracking write
	// so the caller can record access on the final returned set only.
	NoAccessTracking bool
}

type scoredMemory struct {
	m     *Memory
	score float32
}

const (
	searchMaxCandidates = 2000
	searchBatchSize     = 64
)

func (db *DB) SearchMemoriesFilteredWithTrust(queryVec []float32, querySource string, scope string, limit int, entityID string, trustFilter TrustFilter, policyFilter PolicyFilter, timeWindow TimeWindow, quantization string, weights ...RankingWeights) ([]SearchResult, error) {
	w := DefaultRankingWeights()
	if len(weights) > 0 {
		w = weights[0]
	}
	q := &SearchQuery{
		QueryVec:     queryVec,
		QuerySource:  querySource,
		Scope:        scope,
		Limit:        limit,
		EntityID:     entityID,
		TrustFilter:  trustFilter,
		PolicyFilter: policyFilter,
		TimeWindow:   timeWindow,
		Quantization: quantization,
		Weights:      w,
	}
	return db.search(q)
}

// search runs the full retrieval pipeline for a prepared SearchQuery.
func (db *DB) search(q *SearchQuery) ([]SearchResult, error) {
	start := time.Now()

	candidateIDs, err := db.lshCandidateIDs(q)
	if err != nil {
		return nil, err
	}
	if len(candidateIDs) == 0 {
		db.retrievalStats.Record(0, 0, time.Since(start))
		return nil, nil
	}

	results, err := db.hydrateCandidates(q, candidateIDs)
	if err != nil {
		return nil, err
	}
	results = db.applyFilters(q, results)
	results = db.scoreAndRank(q, results)
	results, err = db.applySpreading(q, results)
	if err != nil {
		return nil, err
	}

	limit := q.Limit
	if limit > len(results) {
		limit = len(results)
	}
	final := make([]SearchResult, 0, len(results))
	for i := 0; i < limit; i++ {
		final = append(final, SearchResult{
			Memory: results[i].m,
			Score:  results[i].score,
		})
	}

	latency := time.Since(start)
	if len(final) > 0 {
		db.retrievalStats.Record(len(final), float64(final[0].Score), latency)
	} else {
		db.retrievalStats.Record(0, 0, latency)
	}
	// Track access for retrieval feedback loop (skipped when the caller
	// wants to record access on the final returned set only).
	if !q.NoAccessTracking && len(final) > 0 {
		ids := make([]string, len(final))
		for i, r := range final {
			ids[i] = r.Memory.ID
		}
		_ = db.TrackMemoryAccessBatch(ids) // best-effort; error is non-fatal
	}

	return final, nil
}

// lshCandidateIDs expands the query vector into LSH buckets and returns the
// batched candidate memory IDs, scoped or unscoped.
func (db *DB) lshCandidateIDs(q *SearchQuery) ([]string, error) {
	queryLSH, err := ComputeLSH(q.QueryVec)
	if err != nil {
		return nil, fmt.Errorf("search vector: %w", err)
	}
	buckets := LSHNeighbors(queryLSH, 2)

	twClause, twArgs := TimeWindowClause(q.TimeWindow, "")

	var candidateIDs []string
	for i := 0; i < len(buckets) && len(candidateIDs) < searchMaxCandidates; i += searchBatchSize {
		end := i + searchBatchSize
		if end > len(buckets) {
			end = len(buckets)
		}
		chunk := buckets[i:end]

		placeholders := make([]string, len(chunk))
		args := make([]interface{}, 0, len(chunk)+1)
		for j, h := range chunk {
			placeholders[j] = "?"
			args = append(args, h)
		}
		inClause := strings.Join(placeholders, ", ")

		var query string
		if len(q.Scopes) > 0 {
			scopePlaceholders := make([]string, len(q.Scopes))
			scopeArgs := make([]interface{}, 0, len(q.Scopes)+2)
			for j, s := range q.Scopes {
				scopePlaceholders[j] = "?"
				scopeArgs = append(scopeArgs, s)
			}
			query = "SELECT id FROM memories WHERE scope IN (" + strings.Join(scopePlaceholders, ", ") + ") AND consolidation_status != 'archived' AND embedding_source = ? AND embedding_quantization = ? AND embedding IS NOT NULL AND lsh_hash IN (" + inClause + ") AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL"
			scopeArgs = append(scopeArgs, q.QuerySource, q.Quantization)
			args = append(scopeArgs, args...)
		} else if q.Scope != "" {
			query = "SELECT id FROM memories WHERE scope = ? AND consolidation_status != 'archived' AND embedding_source = ? AND embedding_quantization = ? AND embedding IS NOT NULL AND lsh_hash IN (" + inClause + ") AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL"
			args = append([]interface{}{q.Scope, q.QuerySource, q.Quantization}, args...)
		} else {
			query = "SELECT id FROM memories WHERE consolidation_status != 'archived' AND embedding_source = ? AND embedding_quantization = ? AND embedding IS NOT NULL AND lsh_hash IN (" + inClause + ") AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now')) AND review_status = 'approved' AND retired_at IS NULL"
			args = append([]interface{}{q.QuerySource, q.Quantization}, args...)
		}
		if q.EntityID != "" {
			query += " AND id IN (SELECT memory_id FROM memory_entities WHERE entity_id = ?)"
			args = append(args, q.EntityID)
		}
		query += twClause
		args = append(args, twArgs...)
		query += " ORDER BY created_at DESC"

		rows, err := db.conn.Query(query, args...)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var id string
			if err := rows.Scan(&id); err != nil {
				_ = rows.Close()
				return nil, err
			}
			candidateIDs = append(candidateIDs, id)
			if len(candidateIDs) >= searchMaxCandidates {
				break
			}
		}
		_ = rows.Close()
	}
	return candidateIDs, nil
}

// hydrateCandidates loads the full memory rows for the candidate IDs,
// discarding rows whose embedding dimension does not match the query vector.
func (db *DB) hydrateCandidates(q *SearchQuery, candidateIDs []string) ([]scoredMemory, error) {
	var results []scoredMemory
	for i := 0; i < len(candidateIDs); i += searchBatchSize {
		end := i + searchBatchSize
		if end > len(candidateIDs) {
			end = len(candidateIDs)
		}
		chunk := candidateIDs[i:end]

		placeholders := make([]string, len(chunk))
		args := make([]interface{}, 0, len(chunk))
		for j, id := range chunk {
			placeholders[j] = "?"
			args = append(args, id)
		}
		inClause := strings.Join(placeholders, ", ")

		query := "SELECT " + memoryColumns + " FROM memories WHERE id IN (" + inClause + ")"
		rows, err := db.conn.Query(query, args...)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			m, err := scanMemory(rows)
			if err != nil {
				_ = rows.Close()
				return nil, err
			}
			if len(m.Embedding) != len(q.QueryVec) {
				continue
			}
			if len(m.Embedding) > 0 {
				results = append(results, scoredMemory{m: m})
			}
		}
		_ = rows.Close()
	}
	return results, nil
}

// applyFilters drops candidates that fail the trust, policy or time-window
// filters.
func (db *DB) applyFilters(q *SearchQuery, results []scoredMemory) []scoredMemory {
	filtered := make([]scoredMemory, 0, len(results))
	for _, r := range results {
		if !passesTrustFilter(r.m, q.TrustFilter) {
			continue
		}
		if !PassesPolicyFilter(r.m, q.PolicyFilter) {
			continue
		}
		if !passesTimeWindow(r.m, q.TimeWindow) {
			continue
		}
		filtered = append(filtered, r)
	}
	return filtered
}

// scoreAndRank applies the Hamming prefilter (when enabled), computes the
// composite score for each candidate and sorts descending.
func (db *DB) scoreAndRank(q *SearchQuery, results []scoredMemory) []scoredMemory {
	// Hamming prefilter: reduce cosine computations by selecting candidates
	// closest in sign-bit space. Width is derived from the requested limit;
	// when any candidate lacks a binary vector we skip entirely (see #534).
	if db.prefilterEnabled && len(results) > 0 {
		prefilterN := q.Limit * 4
		if prefilterN < 64 {
			prefilterN = 64
		}
		if prefilterN > len(results) {
			prefilterN = len(results)
		}

		candidateBins := make([][]byte, 0, len(results))
		skipPrefilter := false
		for _, r := range results {
			if r.m.EmbeddingBinary == nil {
				skipPrefilter = true
				break
			}
			candidateBins = append(candidateBins, r.m.EmbeddingBinary)
		}

		if !skipPrefilter {
			queryBin := BinarizeVector(q.QueryVec)
			keepIdx := HammingPrefilter(queryBin, candidateBins, prefilterN)
			filtered := make([]scoredMemory, 0, len(keepIdx))
			for _, idx := range keepIdx {
				filtered = append(filtered, results[idx])
			}
			results = filtered
		}
	}

	for i := range results {
		relevance := CosineSimilarity(q.QueryVec, results[i].m.Embedding)
		decay := results[i].m.DecayFactor
		if decay <= 0 || decay > 1 {
			decay = 1.0
		}
		results[i].score = float32(CompositeScore(relevance, results[i].m.CreatedAt, float64(results[i].m.Importance)/10.0, results[i].m.AccessCount, results[i].m.LastAccess, results[i].m.PrevAccess, q.Weights) * decay)
	}

	sort.Slice(results, func(i, j int) bool {
		return results[i].score > results[j].score
	})
	return results
}

// applySpreading applies memory-association spreading (#488) when enabled,
// hydrating bonus targets that were not LSH candidates and re-sorting.
func (db *DB) applySpreading(q *SearchQuery, results []scoredMemory) ([]scoredMemory, error) {
	if q.Weights.SpreadingWeight <= 0 {
		return results, nil
	}
	db.ensureAssociationsSeeded()
	seeds := make(map[string]float64, 10)
	for i := range results {
		seeds[results[i].m.ID] = float64(results[i].score)
		if len(seeds) >= 10 {
			break
		}
	}
	bonus, err := db.SpreadingBonus(seeds, 32, 32)
	if err != nil {
		return nil, fmt.Errorf("spreading bonus: %w", err)
	}

	inResults := make(map[string]bool, len(results))
	for i := range results {
		inResults[results[i].m.ID] = true
	}
	var bonusIDs []string
	for id, b := range bonus {
		if b > 0 && !inResults[id] {
			bonusIDs = append(bonusIDs, id)
		}
		if len(bonusIDs) >= 50 {
			break
		}
	}
	if len(bonusIDs) > 0 {
		placeholders := make([]string, len(bonusIDs))
		args := make([]interface{}, 0, len(bonusIDs))
		for j, id := range bonusIDs {
			placeholders[j] = "?"
			args = append(args, id)
		}
		rows, err := db.conn.Query(
			"SELECT "+memoryColumns+" FROM memories WHERE id IN ("+strings.Join(placeholders, ", ")+") AND review_status = 'approved' AND retired_at IS NULL",
			args...,
		)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			m, err := scanMemory(rows)
			if err != nil {
				_ = rows.Close()
				return nil, err
			}
			if !passesTrustFilter(m, q.TrustFilter) || !PassesPolicyFilter(m, q.PolicyFilter) || !passesTimeWindow(m, q.TimeWindow) {
				continue
			}
			results = append(results, scoredMemory{m: m, score: float32(q.Weights.SpreadingWeight * bonus[m.ID])})
		}
		_ = rows.Close()
	}

	for i := range results {
		if b, ok := bonus[results[i].m.ID]; ok {
			results[i].score += float32(q.Weights.SpreadingWeight * b)
		}
	}
	sort.Slice(results, func(i, j int) bool {
		return results[i].score > results[j].score
	})
	return results, nil
}

// passesTrustFilter implements trust-aware filtering for search results.
func passesTrustFilter(m *Memory, f TrustFilter) bool {
	if f.ExcludeSuperseded && m.SupersededBy != "" {
		return false
	}

	meta := m.Metadata
	if meta == nil {
		meta = make(map[string]string)
	}

	if f.MinConfidence != "" {
		confidence := meta["confidence"]
		if confidence == "" {
			confidence = "medium"
		}
		if !confidenceMeetsMinimum(confidence, f.MinConfidence) {
			return false
		}
	}

	if f.VerificationStatus != "" {
		status := meta["verification_status"]
		if status == "" {
			status = "unverified"
		}
		if status != f.VerificationStatus {
			return false
		}
	}

	if f.MaxAge > 0 {
		if time.Since(m.CreatedAt) > f.MaxAge {
			return false
		}
	}

	return true
}

func confidenceMeetsMinimum(actual, minimum string) bool {
	rank := map[string]int{
		"low":    0,
		"medium": 1,
		"high":   2,
	}
	actualRank, ok := rank[actual]
	if !ok {
		actualRank = 1
	}
	minRank, ok := rank[minimum]
	if !ok {
		minRank = 1
	}
	return actualRank >= minRank
}

// PassesPolicyFilter checks if a memory passes the given policy filter.
func PassesPolicyFilter(m *Memory, f PolicyFilter) bool {
	if f.MaxSensitivity == "" && f.MinSharingLevel == "" && f.ClientID == "" {
		return true
	}

	meta := m.Metadata
	if meta == nil {
		meta = make(map[string]string)
	}

	if f.MaxSensitivity != "" {
		sensitivity := meta["sensitivity"]
		if sensitivity == "" {
			sensitivity = "internal"
		}
		if !sensitivityWithinLimit(sensitivity, f.MaxSensitivity) {
			return false
		}
	}

	if f.MinSharingLevel != "" {
		sharing := meta["sharing_level"]
		if sharing == "" {
			sharing = "private"
		}
		if !sharingMeetsMinimum(sharing, f.MinSharingLevel) {
			return false
		}
	}

	if f.ClientID != "" {
		allowed := meta["allowed_clients"]
		denied := meta["denied_clients"]
		if denied != "" {
			for _, c := range splitCSV(denied) {
				if c == f.ClientID {
					return false
				}
			}
		}
		if allowed != "" {
			found := false
			for _, c := range splitCSV(allowed) {
				if c == f.ClientID {
					found = true
					break
				}
			}
			if !found {
				return false
			}
		}
	}

	return true
}

func sensitivityWithinLimit(actual, max string) bool {
	rank := map[string]int{
		"public":       0,
		"internal":     1,
		"confidential": 2,
		"secret":       3,
	}
	actualRank, ok := rank[actual]
	if !ok {
		actualRank = 1
	}
	maxRank, ok := rank[max]
	if !ok {
		maxRank = 1
	}
	return actualRank <= maxRank
}

func sharingMeetsMinimum(actual, minimum string) bool {
	rank := map[string]int{
		"private": 0,
		"team":    1,
		"org":     2,
		"public":  3,
	}
	actualRank, ok := rank[actual]
	if !ok {
		actualRank = 0
	}
	minRank, ok := rank[minimum]
	if !ok {
		minRank = 0
	}
	return actualRank >= minRank
}

func splitCSV(s string) []string {
	var result []string
	for _, part := range strings.Split(s, ",") {
		trimmed := strings.TrimSpace(part)
		if trimmed != "" {
			result = append(result, trimmed)
		}
	}
	return result
}

// nullTimePtr returns nil when t is nil, otherwise returns *t.
// Used for nullable time parameters in SQL upserts.
func nullTimePtr(t *time.Time) interface{} {
	if t == nil {
		return nil
	}
	return *t
}

// CosineSimilarity calculates the cosine similarity between two float vectors.
func CosineSimilarity(a, b []float32) float32 {
	if len(a) != len(b) || len(a) == 0 {
		return 0
	}
	var dotProduct, normA, normB float64
	for i := 0; i < len(a); i++ {
		dotProduct += float64(a[i] * b[i])
		normA += float64(a[i] * a[i])
		normB += float64(b[i] * b[i])
	}
	if normA == 0 || normB == 0 {
		return 0
	}
	return float32(dotProduct / (math.Sqrt(normA) * math.Sqrt(normB)))
}

// CompositeScore computes a composite retrieval score: w_r * relevance + w_t * recency + w_i * importance + w_a * access_reinforcement.
// recencyDecay applies exponential decay based on age in days.
// access_reinforcement rewards frequently and recently accessed memories
// (log-scaled count × last-access recency). When prevAccess is set, the
// count term is spacing-aware (#489): the boost scales with the interval
// since the previous reinforcement, so a long-gap recall earns a large
// boost while repeated same-session recalls hit diminishing returns.
func CompositeScore(relevance float32, memoryAge time.Time, importance float64, accessCount int64, lastAccess *time.Time, prevAccess *time.Time, weights RankingWeights) float64 {
	age := time.Since(memoryAge).Hours() / 24.0
	halfLife := weights.RecencyHalfLife
	if halfLife <= 0 {
		halfLife = 30
	}
	recency := math.Exp(-0.693 * age / halfLife)

	// Access reinforcement: boost for frequently and recently accessed memories.
	var accessScore float64
	if weights.AccessReinforcementWeight > 0 {
		accessNorm := math.Log1p(float64(accessCount)) / math.Log1p(100.0) // log-scaled, cap at ~100 accesses
		if accessNorm > 1.0 {
			accessNorm = 1.0
		}
		// Spacing-aware correction (#489): the boost is gated by the gap
		// since the previous reinforcement. Same-session bursts (gap ≈ 0)
		// contribute almost nothing; long-gap recalls (reconfirmation of an
		// old fact) contribute the full count term.
		if lastAccess != nil && prevAccess != nil && !lastAccess.IsZero() && !prevAccess.IsZero() && lastAccess.After(*prevAccess) {
			gapDays := lastAccess.Sub(*prevAccess).Hours() / 24.0
			spacingHalfLife := weights.AccessSpacingHalfLife
			if spacingHalfLife <= 0 {
				spacingHalfLife = 30
			}
			accessNorm *= 1 - math.Exp(-gapDays/spacingHalfLife)
		}
		if lastAccess != nil && !lastAccess.IsZero() {
			daysSinceAccess := time.Since(*lastAccess).Hours() / 24.0
			accessHalfLife := weights.AccessHalfLife
			if accessHalfLife <= 0 {
				accessHalfLife = 14
			}
			lastAccessRecency := math.Exp(-0.693 * daysSinceAccess / accessHalfLife)
			accessScore = accessNorm * lastAccessRecency
		} else {
			// Never accessed or no timestamp — very minimal boost (cold-start fairness).
			accessScore = accessNorm * 0.01
		}
	}

	totalWeight := weights.RelevanceWeight + weights.RecencyWeight + weights.ImportanceWeight + weights.AccessReinforcementWeight
	if totalWeight == 0 {
		totalWeight = 1.0
	}

	score := (weights.RelevanceWeight*float64(relevance) +
		weights.RecencyWeight*recency +
		weights.ImportanceWeight*importance +
		weights.AccessReinforcementWeight*accessScore) / totalWeight

	return score
}

// RankSearchResults re-ranks search results using composite scoring.
func RankSearchResults(results []SearchResult, weights RankingWeights) []SearchResult {
	if len(results) == 0 {
		return results
	}
	type scoredResult struct {
		result    SearchResult
		composite float64
	}
	scored := make([]scoredResult, len(results))
	for i, r := range results {
		scored[i] = scoredResult{
			result:    r,
			composite: CompositeScore(r.Score, r.Memory.UpdatedAt, r.Memory.Importance, r.Memory.AccessCount, r.Memory.LastAccess, r.Memory.PrevAccess, weights),
		}
	}

	sort.Slice(scored, func(i, j int) bool {
		return scored[i].composite > scored[j].composite
	})

	ranked := make([]SearchResult, len(scored))
	for i, s := range scored {
		ranked[i] = s.result
	}
	return ranked
}

// FactExists checks if a memory with the given content hash already exists.
func (db *DB) FactExists(contentHash string) (bool, error) {
	var count int
	err := db.conn.QueryRow(
		"SELECT 1 FROM memories WHERE content_hash = ? LIMIT 1",
		contentHash,
	).Scan(&count)
	if err == sql.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

// FindActiveDuplicate returns the newest active memory in the given scope
// with the exact same content hash, or nil when none exists. Active means
// live in retrieval: not archived by consolidation, not staged, not
// retired and not already superseded. The lookup does not track access.
func (db *DB) FindActiveDuplicate(contentHash, scope string) (*Memory, error) {
	if contentHash == "" {
		return nil, nil
	}
	var id string
	err := db.conn.QueryRow(`SELECT id FROM memories
		WHERE content_hash = ? AND scope = ?
		  AND consolidation_status != 'archived'
		  AND review_status = 'approved'
		  AND retired_at IS NULL
		  AND (superseded_by IS NULL OR superseded_by = '')
		ORDER BY created_at DESC LIMIT 1`, contentHash, scope).Scan(&id)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return db.loadMemory(id)
}

// SupersedeMemory marks the loser as superseded by the winner and closes
// its validity window at validTo. The update is guarded: a memory that is
// already superseded is never re-litigated, and a missing loser is a
// no-op rather than an error (the winner stands either way).
func (db *DB) SupersedeMemory(loserID, winnerID string, validTo time.Time, updatedBy, updatedSession string) error {
	_, err := db.conn.Exec(`UPDATE memories
		SET superseded_by = ?, valid_to = ?, updated_at = ?, updated_by = ?, updated_session = ?
		WHERE id = ? AND (superseded_by IS NULL OR superseded_by = '')`,
		winnerID, validTo.UTC(), time.Now().UTC(), nullStr(updatedBy), nullStr(updatedSession), loserID)
	return err
}

// SetMemoryEmbedding updates only the embedding columns of an existing memory,
// leaving all other fields (identity, timestamps, content, metadata) untouched.
func (db *DB) SetMemoryEmbedding(id string, embedding []float32, source, model, quantization string) error {
	embeddingJSON, err := json.Marshal(embedding)
	if err != nil {
		return fmt.Errorf("failed to marshal embedding: %w", err)
	}
	embeddingDim := len(embedding)
	lshHash, err := ComputeLSH(embedding)
	if err != nil {
		return err
	}
	var embBin []byte
	if db.quantizeBinary && len(embedding) > 0 {
		embBin = BinarizeVector(embedding)
	}

	_, err = db.conn.Exec(
		`UPDATE memories SET embedding = ?, embedding_binary = ?, embedding_dim = ?, embedding_source = ?, embedding_model = ?, embedding_quantization = ?, lsh_hash = ? WHERE id = ?`,
		string(embeddingJSON), embBin, embeddingDim, source, model, quantization, lshHash, id,
	)
	return err
}

// SupersedeFact marks an older memory as superseded by a newer one.
// Sets valid_to on the superseded memory and records the superseding ID.
func (db *DB) SupersedeFact(supersededID, supersededByID string) error {
	now := time.Now().UTC()
	_, err := db.conn.Exec(
		`UPDATE memories SET valid_to = ?, superseded_by = ?, updated_at = ? WHERE id = ? AND (valid_to IS NULL OR valid_to > ?)`,
		now, supersededByID, now, supersededID, now,
	)
	return err
}

// GetWorkingMemories returns active (non-expired) working memories for the given scope.
func (db *DB) GetWorkingMemories(scope string, limit int) ([]*Memory, error) {
	if limit <= 0 {
		limit = 50
	}
	query := "SELECT " + memoryColumnsLite + " FROM memories WHERE tier = 'working' AND (expires_at IS NULL OR expires_at > datetime('now')) AND consolidation_status != 'archived'"
	var args []interface{}
	if scope != "" {
		query += " AND scope = ?"
		args = append(args, scope)
	}
	query += " ORDER BY created_at DESC LIMIT ?"
	args = append(args, limit)

	rows, err := db.conn.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}

// GetExpiredWorkingMemories returns working memories whose expires_at has passed.
func (db *DB) GetExpiredWorkingMemories() ([]*Memory, error) {
	query := "SELECT " + memoryColumnsLite + " FROM memories WHERE tier = 'working' AND expires_at IS NOT NULL AND expires_at <= datetime('now') AND consolidation_status != 'archived' ORDER BY expires_at ASC"

	rows, err := db.conn.Query(query)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}

// EvictExpiredWorkingMemories deletes expired working memories and returns the count of deleted rows.
func (db *DB) EvictExpiredWorkingMemories() (int64, error) {
	result, err := db.conn.Exec(
		"DELETE FROM memories WHERE tier = 'working' AND expires_at IS NOT NULL AND expires_at <= datetime('now')",
	)
	if err != nil {
		return 0, err
	}
	return result.RowsAffected()
}

// SetMemoryTier updates the tier and optionally the expires_at of a memory.
// Pass nil for expiresAt to clear the expiry (e.g. when demoting back to long_term).
func (db *DB) SetMemoryTier(id, tier string, expiresAt *time.Time) error {
	var exp sql.NullTime
	if expiresAt != nil {
		exp.Time = *expiresAt
		exp.Valid = true
	}
	_, err := db.conn.Exec(
		"UPDATE memories SET tier = ?, expires_at = ?, updated_at = ? WHERE id = ?",
		tier, exp, time.Now().UTC(), id,
	)
	return err
}

// TrackMemoryAccess increments the access_count and updates last_access for
// the given memory. This is the core of the retrieval feedback loop. The
// previous last_access is shifted into prev_access so the reinforcement
// curve can measure the gap between recalls (#489).
func (db *DB) TrackMemoryAccess(id string) error {
	now := time.Now().UTC()
	_, err := db.conn.Exec(
		"UPDATE memories SET access_count = access_count + 1, prev_access = last_access, last_access = ? WHERE id = ?",
		now, id,
	)
	return err
}

// TrackMemoryAccessBatch increments access_count and updates last_access for
// multiple memories in a single SQL statement. Uses an IN clause for
// efficient batch updates. prev_access receives the previous last_access so
// the spacing-aware reinforcement curve has the recall gap (#489).
func (db *DB) TrackMemoryAccessBatch(ids []string) error {
	if len(ids) == 0 {
		return nil
	}
	now := time.Now().UTC()
	placeholders := make([]string, len(ids))
	args := make([]interface{}, 0, len(ids)+1)
	args = append(args, now)
	for i, id := range ids {
		placeholders[i] = "?"
		args = append(args, id)
	}
	inClause := strings.Join(placeholders, ", ")
	_, err := db.conn.Exec(
		"UPDATE memories SET access_count = access_count + 1, prev_access = last_access, last_access = ? WHERE id IN ("+inClause+")",
		args...,
	)
	return err
}

// SearchMemoriesWithProfile resolves the given context profile into an ordered
// list of scopes, searches each scope in precedence order, and returns results
// tagged with provenance. When profileName is empty, it falls back to the
// single-scope behaviour of SearchMemoriesFilteredWithTrust.
func (db *DB) SearchMemoriesWithProfile(queryVec []float32, querySource string, profileName string, limit int, entityID string, trustFilter TrustFilter, policyFilter PolicyFilter, timeWindow TimeWindow, weights ...RankingWeights) ([]SearchResult, error) {
	if profileName == "" {
		return db.SearchMemoriesFilteredWithTrust(queryVec, querySource, "", limit, entityID, trustFilter, policyFilter, timeWindow, "", weights...)
	}

	scopes, err := db.ResolveContextProfile(profileName, DefaultMaxDepth)
	if err != nil {
		return nil, err
	}

	w := DefaultRankingWeights()
	if len(weights) > 0 {
		w = weights[0]
	}

	// Collect unique scopes into a single ordered list for one candidate pass.
	seen := make(map[string]bool)
	var scopeList []string
	for _, s := range scopes {
		if !seen[s.Scope] {
			seen[s.Scope] = true
			scopeList = append(scopeList, s.Scope)
		}
	}

	// Single pipeline pass over the whole scope chain. Access tracking is
	// deferred to this caller so only the final returned set is recorded.
	q := &SearchQuery{
		QueryVec:         queryVec,
		QuerySource:      querySource,
		Scopes:           scopeList,
		Limit:            limit * 2,
		EntityID:         entityID,
		TrustFilter:      trustFilter,
		PolicyFilter:     policyFilter,
		TimeWindow:       timeWindow,
		Quantization:     "",
		Weights:          w,
		NoAccessTracking: true,
	}
	results, err := db.search(q)
	if err != nil {
		return nil, err
	}

	// Tag provenance from the hydrated row (each memory knows its own scope).
	for i := range results {
		results[i].SourceProfile = profileName
		results[i].SourceScope = results[i].Memory.Scope
	}

	if limit < len(results) {
		results = results[:limit]
	}

	// Record access only for the final returned set.
	if len(results) > 0 {
		ids := make([]string, len(results))
		for i, r := range results {
			ids[i] = r.Memory.ID
		}
		_ = db.TrackMemoryAccessBatch(ids)
	}

	return results, nil
}

// GetSupersededHistory returns all memories that were superseded by the given ID.
func (db *DB) GetSupersededHistory(supersededByID string) ([]*Memory, error) {
	rows, err := db.conn.Query(
		"SELECT "+memoryColumnsLite+" FROM memories WHERE superseded_by = ? ORDER BY valid_from DESC",
		supersededByID,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	return memories, nil
}
