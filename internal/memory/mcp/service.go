package mcp

import (
	"fmt"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/conflict"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/memory"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// MemoryService encapsulates all business logic for memory operations,
// decoupling HTTP handlers from database and extraction internals.
type MemoryService struct {
	db         *db.DB
	extractor  *extractor.PatternExtractor
	embeddings *extractor.EmbeddingsGenerator
	piiEnabled bool
	weights    db.RankingWeights // retrieval ranking weights (#488/#489 config)

	// conflictChecker runs write-path contradiction detection (#462).
	// nil keeps the legacy write behavior (unconditional inserts).
	conflictChecker *conflict.Checker
}

// NewMemoryService creates a service with the given dependencies.
func NewMemoryService(database *db.DB, embeddings *extractor.EmbeddingsGenerator, piiEnabled bool) *MemoryService {
	return &MemoryService{
		db:         database,
		extractor:  extractor.NewPatternExtractor(),
		embeddings: embeddings,
		piiEnabled: piiEnabled,
		weights:    db.DefaultRankingWeights(),
	}
}

// SetRankingWeights applies the configured retrieval weights.
func (s *MemoryService) SetRankingWeights(w db.RankingWeights) {
	s.weights = w
}

// SetConflictChecker enables write-path contradiction detection (#462).
// A nil checker restores the legacy write behavior (unconditional
// inserts, no dedup, no supersession).
func (s *MemoryService) SetConflictChecker(cc *conflict.Checker) {
	s.conflictChecker = cc
}

// checkerForStaged returns the conflict checker to use for a write that
// will land as a staged candidate. Staged writes bypass the check: a
// candidate awaiting review must never supersede a live fact.
func (s *MemoryService) checkerForStaged(staged bool) *conflict.Checker {
	if staged {
		return nil
	}
	return s.conflictChecker
}

func (s *MemoryService) SetPIIEnabled(enabled bool) {
	s.piiEnabled = enabled
}

func (s *MemoryService) ActiveBackend() string {
	return s.embeddings.ActiveBackend()
}

// EmbeddingMetrics returns live embedding generator metrics.
func (s *MemoryService) EmbeddingMetrics() extractor.EmbeddingMetrics {
	return s.embeddings.Metrics()
}

// RetrievalStats returns the live retrieval stats accumulator.
func (s *MemoryService) RetrievalStats() *db.RetrievalStats {
	return s.db.RetrievalStats()
}

func (s *MemoryService) Search(query, scope string, limit int, entityName string, trustFilter db.TrustFilter, policyFilter db.PolicyFilter, timeWindow ...db.TimeWindow) ([]db.SearchResult, error) {
	var entityID string
	if entityName != "" {
		entity, err := s.db.ResolveEntity(entityName)
		if err != nil {
			return nil, fmt.Errorf("resolve entity: %w", err)
		}
		if entity == nil {
			return nil, &NotFoundError{Resource: "entity", Identifier: entityName}
		}
		entityID = entity.ID
	}

	tw := db.TimeWindow{}
	if len(timeWindow) > 0 {
		tw = timeWindow[0]
	}

	emb := s.embeddings.GenerateVector(query)
	return s.db.SearchMemoriesFilteredWithTrust(emb.Vector, emb.Source, scope, limit, entityID, trustFilter, policyFilter, tw, "", s.weights)
}

// HybridSearch performs hybrid vector + BM25 retrieval through the HybridSearch
// path, returning HybridResult. It resolves entity names and generates embeddings.
// When hybrid is disabled in config, callers should fall back to Search().
func (s *MemoryService) HybridSearch(query, scope string, limit int, entityName string, trustFilter db.TrustFilter, policyFilter db.PolicyFilter, vectorWeight, bm25Weight float64, timeWindow ...db.TimeWindow) ([]db.HybridResult, error) {
	var entityID string
	if entityName != "" {
		entity, err := s.db.ResolveEntity(entityName)
		if err != nil {
			return nil, fmt.Errorf("resolve entity: %w", err)
		}
		if entity == nil {
			return nil, &NotFoundError{Resource: "entity", Identifier: entityName}
		}
		entityID = entity.ID
	}

	emb := s.embeddings.GenerateVector(query)
	return s.db.HybridSearchWithWeights(emb.Vector, emb.Source, query, scope, limit, entityID, trustFilter, policyFilter, vectorWeight, bm25Weight, s.weights, timeWindow...)
}
func (s *MemoryService) SearchWithProfile(query, profileName string, limit int, entityName string, trustFilter db.TrustFilter, policyFilter db.PolicyFilter, timeWindow ...db.TimeWindow) ([]db.SearchResult, error) {
	var entityID string
	if entityName != "" {
		entity, err := s.db.ResolveEntity(entityName)
		if err != nil {
			return nil, fmt.Errorf("resolve entity: %w", err)
		}
		if entity == nil {
			return nil, &NotFoundError{Resource: "entity", Identifier: entityName}
		}
		entityID = entity.ID
	}

	tw := db.TimeWindow{}
	if len(timeWindow) > 0 {
		tw = timeWindow[0]
	}

	emb := s.embeddings.GenerateVector(query)
	return s.db.SearchMemoriesWithProfile(emb.Vector, emb.Source, profileName, limit, entityID, trustFilter, policyFilter, tw, s.weights)
}

func (s *MemoryService) Set(content, scope string, metadata map[string]string, sessionID string, author string, entities []string, sourceTool string, working bool, ttl time.Duration) (string, error) {
	return s.set(content, scope, metadata, sessionID, author, entities, sourceTool, working, ttl, s.conflictChecker)
}

func (s *MemoryService) set(content, scope string, metadata map[string]string, sessionID string, author string, entities []string, sourceTool string, working bool, ttl time.Duration, checker *conflict.Checker) (string, error) {
	attr := memory.Attribution{
		Author:    author,
		SessionID: sessionID,
	}
	m, _, err := memory.Store(s.db, s.embeddings, s.extractor, content, scope, metadata, s.piiEnabled, attr, entities, sourceTool, working, ttl, memory.StoreOptions{
		ConflictChecker: checker,
	})
	if err != nil {
		return "", err
	}
	return m.ID, nil
}

// SetGoverned stores a memory like Set and then applies write-path
// governance (#485/#486): the canonical semantic kind (validated against
// the four buckets) and the review state. Staged writes land as
// candidates excluded from retrieval until memory_promote approves them.
func (s *MemoryService) SetGoverned(content, scope string, metadata map[string]string, sessionID string, author string, entities []string, sourceTool string, working bool, ttl time.Duration, kind string, staged bool) (string, error) {
	canonical, ok := db.NormalizeKind(kind)
	if !ok {
		return "", fmt.Errorf("invalid kind %q (valid: %s)", kind, strings.Join(db.ValidKinds(), ", "))
	}
	id, err := s.set(content, scope, metadata, sessionID, author, entities, sourceTool, working, ttl, s.checkerForStaged(staged))
	if err != nil {
		return "", err
	}
	if err := s.db.SetMemoryKind(id, canonical); err != nil {
		return "", err
	}
	if staged {
		if err := s.db.SetMemoryReviewStatus(id, db.ReviewStaged); err != nil {
			return "", err
		}
	}
	return id, nil
}

// Candidates returns the staged review queue (#485), newest first.
func (s *MemoryService) Candidates(limit int) ([]*db.Memory, error) {
	return s.db.ListStagedMemories(limit)
}

// Promote approves a staged candidate so it becomes retrievable (#485).
func (s *MemoryService) Promote(id string) error {
	m, err := s.db.GetMemory(id)
	if err != nil {
		return err
	}
	if m == nil {
		return &NotFoundError{Resource: "memory", Identifier: id}
	}
	if err := s.db.SetMemoryReviewStatus(id, db.ReviewApproved); err != nil {
		return err
	}
	_ = s.db.LogAudit("promote", id, m.Scope, m.CreatedSession, m.CreatedBy, "")
	return nil
}

// Reject removes a staged candidate (#485). A rejected candidate is
// deleted: it never entered the curated store, so there is nothing to
// keep. Live memories are refused, not silently deleted.
func (s *MemoryService) Reject(id string) error {
	m, err := s.db.GetMemory(id)
	if err != nil {
		return err
	}
	if m == nil {
		return &NotFoundError{Resource: "memory", Identifier: id}
	}
	if m.ReviewStatus != db.ReviewStaged {
		return fmt.Errorf("memory %s is not a staged candidate (review_status=%s)", id, m.ReviewStatus)
	}
	_ = s.db.LogAudit("reject", id, m.Scope, m.CreatedSession, m.CreatedBy, "")
	return s.db.DeleteMemory(id)
}

func (s *MemoryService) Get(id string) (*db.Memory, error) {
	m, err := s.db.GetMemory(id)
	if err != nil {
		return nil, err
	}
	if m == nil {
		return nil, &NotFoundError{Resource: "memory", Identifier: id}
	}
	return m, nil
}

func (s *MemoryService) Delete(id string) error {
	m, err := s.db.GetMemory(id)
	if err != nil {
		return err
	}
	if m == nil {
		return &NotFoundError{Resource: "memory", Identifier: id}
	}
	return s.db.DeleteMemory(id)
}

func (s *MemoryService) List(scope string, limit int) ([]*db.Memory, error) {
	return s.db.ListMemoriesLite(scope, 0, limit)
}

func (s *MemoryService) ListWithPolicy(scope string, limit int, policyFilter db.PolicyFilter) ([]*db.Memory, error) {
	if policyFilter.MaxSensitivity == "" && policyFilter.MinSharingLevel == "" && policyFilter.ClientID == "" {
		return s.db.ListMemoriesLite(scope, 0, limit)
	}
	return s.db.ListMemoriesFilteredWithPolicy(scope, 0, limit, policyFilter)
}

func (s *MemoryService) ListRules(scope string) ([]*db.Rule, error) {
	return s.db.ListRules(scope)
}

func (s *MemoryService) ListEntities() ([]*db.Entity, error) {
	return s.db.ListEntities()
}

func (s *MemoryService) ResolveEntity(nameOrAlias string) (*db.Entity, error) {
	return s.db.ResolveEntity(nameOrAlias)
}

func (s *MemoryService) ResolveEntityCandidates(query, entityType string, aliasHints []string, limit int) ([]db.EntityCandidate, error) {
	return s.db.ResolveEntityCandidates(query, entityType, aliasHints, limit)
}

func (s *MemoryService) GetEntityByID(id string) (*db.Entity, error) {
	return s.db.GetEntityByID(id)
}

func (s *MemoryService) SaveEntityRelationProvenance(r *db.EntityRelation) (*db.EntityRelation, error) {
	return s.db.SaveEntityRelationProvenance(r)
}

func (s *MemoryService) SaveEntityRelation(r *db.EntityRelation) error {
	return s.db.SaveEntityRelation(r)
}

func (s *MemoryService) DeleteEntityRelation(fromEntityID, toEntityID, relationType string) error {
	return s.db.DeleteEntityRelation(fromEntityID, toEntityID, relationType)
}

func (s *MemoryService) GraphNeighbors(entityID string, depth int) ([]*db.Entity, []*db.EntityRelation, error) {
	return s.db.GraphNeighbors(entityID, depth)
}

func (s *MemoryService) GraphNeighborsAsOf(entityID string, depth int, asOf *time.Time) ([]*db.Entity, []*db.EntityRelation, error) {
	return s.db.GraphNeighborsAsOf(entityID, depth, asOf)
}

func (s *MemoryService) ListMemoriesAsOf(scope string, asOf time.Time, limit int) ([]*db.Memory, error) {
	return s.db.ListMemoriesAsOf(scope, asOf, 0, limit)
}

func (s *MemoryService) ListMemoriesAsOfWithCursor(scope string, asOf time.Time, cursor *db.MemoryCursor, limit int) ([]*db.Memory, error) {
	return s.db.ListMemoriesAsOfWithCursor(scope, asOf, cursor, limit)
}

func (s *MemoryService) ListMemoriesLiteWithCursor(scope string, cursor *db.MemoryCursor, limit int) ([]*db.Memory, error) {
	return s.db.ListMemoriesLiteWithCursor(scope, cursor, limit)
}

func (s *MemoryService) GetMemory(id string) (*db.Memory, error) {
	return s.db.GetMemory(id)
}

func (s *MemoryService) GetMemoryEvidence(memoryID string) ([]db.EvidenceSpan, error) {
	return s.db.GetMemoryEvidence(memoryID)
}

func (s *MemoryService) GetMemoriesSinceCursor(since time.Time, limit int, includeEmbeddings ...bool) ([]*db.Memory, error) {
	return s.db.GetMemoriesSinceCursor(since, limit, includeEmbeddings...)
}

func (s *MemoryService) UpsertMemoryIfNewer(m *db.Memory) (bool, error) {
	if s.piiEnabled {
		m.Content = security.Redact(m.Content)
		m.Metadata = security.RedactMap(m.Metadata)
	}
	return s.db.UpsertMemoryIfNewer(m)
}

// SyncUpsertMemoryIfNewer is the tombstone-aware upsert used by sync
// ingestion; it never resurrects a memory deleted after the incoming row.
func (s *MemoryService) SyncUpsertMemoryIfNewer(m *db.Memory) (bool, error) {
	if s.piiEnabled {
		m.Content = security.Redact(m.Content)
		m.Metadata = security.RedactMap(m.Metadata)
	}
	return s.db.SyncUpsertMemoryIfNewer(m)
}

func (s *MemoryService) GetDeletedSince(since time.Time) ([]db.DeletedMemory, error) {
	return s.db.GetDeletedSince(since)
}

func (s *MemoryService) ApplyRemoteDelete(id string, deletedAt time.Time) (bool, error) {
	return s.db.ApplyRemoteDelete(id, deletedAt)
}

func (s *MemoryService) StoreRelayBlob(b db.RelayBlob) (bool, error) {
	return s.db.StoreRelayBlob(b)
}

func (s *MemoryService) GetRelayBlobsSince(since time.Time, limit int) ([]db.RelayBlob, error) {
	return s.db.GetRelayBlobsSince(since, limit)
}

// LogAudit records an audit event. The parameter names and order match
// db.LogAudit exactly: (action, memoryID, scope, session, actor, detail).
// The detail field must never carry memory content or matched PII values —
// only pattern labels or non-sensitive summaries.
func (s *MemoryService) LogAudit(action, memoryID, scope, session, actor, detail string) error {
	return s.db.LogAudit(action, memoryID, scope, session, actor, detail)
}

func (s *MemoryService) LogQuery(actor, scope, session, tool, queryText, params string, durationMs int64) (string, error) {
	return s.db.LogQuery(actor, scope, session, tool, queryText, params, durationMs)
}

// RecordQueryResults links a logged query to the memories its retrieval
// returned (ids and scores only). Best-effort telemetry: callers treat an
// error as non-fatal.
func (s *MemoryService) RecordQueryResults(queryID string, results []db.QueryResultRef) error {
	return s.db.RecordQueryResults(queryID, results)
}

func (s *MemoryService) GetQueryLogEntries(limit int) ([]*db.QueryLogEntry, error) {
	return s.db.GetQueryLogEntries(limit)
}

func (s *MemoryService) GetQueryLogSummary(limit int, actor string) (*db.QueryLogSummary, error) {
	return s.db.GetQueryLogSummary(limit, actor)
}

// NotFoundError indicates a requested resource does not exist.
type NotFoundError struct {
	Resource   string
	Identifier string
}

func (e *NotFoundError) Error() string {
	return fmt.Sprintf("%s not found: %s", e.Resource, e.Identifier)
}
