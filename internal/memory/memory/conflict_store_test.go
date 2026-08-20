package memory

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"math"
	"math/bits"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/conflict"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

// conflictTestSource is the embedding source used for engineered vectors.
const conflictTestSource = "conflict-store-test"

// bandVec returns a unit vector with cosine ≈ target against the unit
// base (1,0,0,...) whose LSH hash is within Hamming distance 2 of the
// base's hash, so the candidate is deterministically recalled.
func bandVec(t *testing.T, target float32) []float32 {
	t.Helper()
	base := make([]float32, db.EmbeddingDim)
	base[0] = 1.0
	baseHash, err := db.ComputeLSH(base)
	if err != nil {
		t.Fatalf("ComputeLSH: %v", err)
	}
	orth := float32(math.Sqrt(1 - float64(target)*float64(target)))
	for j := 1; j < 64; j++ {
		v := make([]float32, db.EmbeddingDim)
		v[0] = target
		v[j] = orth
		h, err := db.ComputeLSH(v)
		if err != nil {
			t.Fatalf("ComputeLSH: %v", err)
		}
		if bits.OnesCount(uint(h^baseHash)) <= 2 {
			return v
		}
	}
	t.Fatalf("no band vector for %v", target)
	return nil
}

// saveEngineered stores a memory with an engineered embedding directly.
func saveEngineered(t *testing.T, database *db.DB, id, content string, emb []float32, importance float64, createdAt time.Time) *db.Memory {
	t.Helper()
	m := &db.Memory{
		ID:              id,
		Content:         content,
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       emb,
		EmbeddingSource: conflictTestSource,
		CreatedBy:       "actor-" + id,
		CreatedSession:  "sess-" + id,
		CreatedAt:       createdAt,
		UpdatedAt:       createdAt,
		Importance:      importance,
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory(%s): %v", id, err)
	}
	return m
}

// portFakeProvider classifies band pairs by content: candidates about a
// port are contradictions, everything else is a repeat.
type portFakeProvider struct{}

func (portFakeProvider) Verdicts(_ context.Context, pairs []conflict.Pair) ([]conflict.Verdict, error) {
	out := make([]conflict.Verdict, len(pairs))
	for i, p := range pairs {
		if strings.Contains(p.OldContent, "port") {
			out[i] = conflict.VerdictContradiction
		} else {
			out[i] = conflict.VerdictRepeat
		}
	}
	return out, nil
}

func conflictCfg() config.ConflictConfig {
	return config.ConflictConfig{
		Enabled:                true,
		ContradictionThreshold: 0.80,
		NearDupThreshold:       0.95,
		MaxCandidates:          10,
	}
}

func rowCount(t *testing.T, database *db.DB) int {
	t.Helper()
	var n int
	if err := database.Conn().QueryRow("SELECT COUNT(*) FROM memories").Scan(&n); err != nil {
		t.Fatalf("count: %v", err)
	}
	return n
}

// ---------------------------------------------------------------------------
// Full Store pipeline tests (hash-fallback embeddings are deterministic).
// ---------------------------------------------------------------------------

func TestStoreDedupByteIdentical(t *testing.T) {
	database := helperMemDB(t)
	embeddings := extractor.NewEmbeddingsGenerator(config.Defaults())
	patternExtractor := extractor.NewPatternExtractor()
	attr := Attribution{Author: "test-user", SessionID: "sess-1"}
	opts := memoryStoreOptions(t, database)

	content := "alice prefers dark mode in all applications"
	m1, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0, opts)
	if err != nil {
		t.Fatalf("first Store: %v", err)
	}
	m2, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0, opts)
	if err != nil {
		t.Fatalf("second Store: %v", err)
	}
	if m2.ID != m1.ID {
		t.Errorf("byte-identical repeat must dedup to the existing row: got %s, want %s", m2.ID, m1.ID)
	}
	if n := rowCount(t, database); n != 1 {
		t.Errorf("expected 1 row after a deduplicated repeat, got %d", n)
	}
}

func TestStoreNearIdenticalReplacesOlderRow(t *testing.T) {
	database := helperMemDB(t)
	embeddings := extractor.NewEmbeddingsGenerator(config.Defaults())
	patternExtractor := extractor.NewPatternExtractor()
	attr := Attribution{Author: "test-user", SessionID: "sess-1"}
	opts := memoryStoreOptions(t, database)

	// The hash-fallback embedding ignores stop words, so these two
	// contents produce identical vectors (cosine 1.0 ≥ near-dup 0.95):
	// same fact region, so the newer representation replaces the older
	// row — the write-path counterpart of consolidation's merge.
	m1, _, err := Store(database, embeddings, patternExtractor, "the daemon runs on port 8787", "global", nil, false, attr, nil, "test", false, 0, opts)
	if err != nil {
		t.Fatalf("first Store: %v", err)
	}
	m2, _, err := Store(database, embeddings, patternExtractor, "daemon runs on port 8787", "global", nil, false, attr, nil, "test", false, 0, opts)
	if err != nil {
		t.Fatalf("second Store: %v", err)
	}
	if m2.ID == m1.ID {
		t.Fatal("a near-identical rewrite must create its own row (newer representation wins)")
	}
	oldRow, err := database.GetMemory(m1.ID)
	if err != nil {
		t.Fatalf("load old row: %v", err)
	}
	if oldRow.SupersededBy != m2.ID {
		t.Errorf("older row must be superseded by the newer representation, got %q", oldRow.SupersededBy)
	}
	if oldRow.ValidTo == nil {
		t.Error("older row's valid_to must be closed")
	}
	// Byte-identical rewrites still dedup outright (hash tier).
	m3, _, err := Store(database, embeddings, patternExtractor, "daemon runs on port 8787", "global", nil, false, attr, nil, "test", false, 0, opts)
	if err != nil {
		t.Fatalf("third Store: %v", err)
	}
	if m3.ID != m2.ID {
		t.Errorf("byte-identical rewrite must dedup to %s, got %s", m2.ID, m3.ID)
	}
	if n := rowCount(t, database); n != 2 {
		t.Errorf("expected 2 rows (one superseded), got %d", n)
	}
}

func TestStoreDisabledRestoresLegacyInsert(t *testing.T) {
	database := helperMemDB(t)
	embeddings := extractor.NewEmbeddingsGenerator(config.Defaults())
	patternExtractor := extractor.NewPatternExtractor()
	attr := Attribution{Author: "test-user", SessionID: "sess-1"}

	cfg := conflictCfg()
	cfg.Enabled = false
	opts := StoreOptions{ConflictChecker: conflict.NewChecker(database, cfg)}

	content := "alice prefers dark mode in all applications"
	if _, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0, opts); err != nil {
		t.Fatalf("first Store: %v", err)
	}
	if _, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0, opts); err != nil {
		t.Fatalf("second Store: %v", err)
	}
	if n := rowCount(t, database); n != 2 {
		t.Errorf("disabled check must restore unconditional inserts: expected 2 rows, got %d", n)
	}
}

func TestStoreWorkingSkipsConflictCheck(t *testing.T) {
	database := helperMemDB(t)
	embeddings := extractor.NewEmbeddingsGenerator(config.Defaults())
	patternExtractor := extractor.NewPatternExtractor()
	attr := Attribution{Author: "test-user", SessionID: "sess-1"}
	opts := memoryStoreOptions(t, database)

	content := "alice prefers dark mode in all applications"
	if _, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0, opts); err != nil {
		t.Fatalf("long-term Store: %v", err)
	}
	if _, _, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", true, time.Hour, opts); err != nil {
		t.Fatalf("working Store: %v", err)
	}
	if n := rowCount(t, database); n != 2 {
		t.Errorf("working memories must bypass the check: expected 2 rows, got %d", n)
	}
}

// ---------------------------------------------------------------------------
// storeWithConflictCheck tests with engineered embeddings (deterministic
// band similarity and recall).
// ---------------------------------------------------------------------------

func memoryStoreOptions(t *testing.T, database *db.DB) StoreOptions {
	t.Helper()
	return StoreOptions{ConflictChecker: conflict.NewChecker(database, conflictCfg())}
}

func TestStoreWithConflictCheckAmbiguousStoresBothAndAudits(t *testing.T) {
	database := helperMemDB(t)
	now := time.Now().UTC()
	saveEngineered(t, database, "old-1", "the daemon listens on port 8787", bandVec(t, 0.90), 0.5, now.Add(-time.Hour))

	attr := Attribution{Author: "writer-a", SessionID: "sess-a"}
	m := &db.Memory{
		ID:              "new-1",
		Content:         "the daemon runs on port 9000",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       bandVecBase(),
		EmbeddingSource: conflictTestSource,
		CreatedBy:       attr.Author,
		CreatedSession:  attr.SessionID,
	}

	checker := conflict.NewChecker(database, conflictCfg()) // no verdict provider
	stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
	if err != nil {
		t.Fatalf("storeWithConflictCheck: %v", err)
	}
	if deduped {
		t.Fatal("ambiguous pair must not be deduplicated")
	}
	if stored.ID != "new-1" {
		t.Fatalf("expected the new memory stored, got %s", stored.ID)
	}
	if n := rowCount(t, database); n != 2 {
		t.Fatalf("ambiguous pair must store both rows, got %d", n)
	}
	oldRow, err := database.GetMemory("old-1")
	if err != nil {
		t.Fatalf("load old: %v", err)
	}
	if oldRow.SupersededBy != "" {
		t.Errorf("ambiguous pair must not supersede, got superseded_by=%q", oldRow.SupersededBy)
	}

	// The disagreement is surfaced in the audit log.
	events, err := database.GetAuditLogs(conflict.ActionConflictPending, 10)
	if err != nil {
		t.Fatalf("GetAuditLogs: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected one conflict_pending event, got %d", len(events))
	}
	if events[0].MemoryID != "new-1" || events[0].Actor != "writer-a" {
		t.Errorf("event attribution wrong: %+v", events[0])
	}
	var detail struct {
		CandidateID    string  `json:"candidate_id"`
		CandidateActor string  `json:"candidate_actor"`
		Similarity     float32 `json:"similarity"`
	}
	if err := json.Unmarshal([]byte(events[0].Detail), &detail); err != nil {
		t.Fatalf("detail JSON: %v", err)
	}
	if detail.CandidateID != "old-1" || detail.CandidateActor != "actor-old-1" {
		t.Errorf("detail names the candidate: %+v", detail)
	}
}

func TestStoreWithConflictCheckContradictionResolves(t *testing.T) {
	database := helperMemDB(t)
	now := time.Now().UTC()
	saveEngineered(t, database, "old-1", "the daemon listens on port 8787", bandVec(t, 0.90), 0.5, now.Add(-time.Hour))

	attr := Attribution{Author: "writer-a", SessionID: "sess-a"}
	m := &db.Memory{
		ID:              "new-1",
		Content:         "the daemon runs on port 9000",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       bandVecBase(),
		EmbeddingSource: conflictTestSource,
		CreatedBy:       attr.Author,
		CreatedSession:  attr.SessionID,
		Importance:      0.5, // equal to the candidate → recency decides
	}

	checker := conflict.NewChecker(database, conflictCfg())
	checker.SetVerdictProvider(portFakeProvider{})
	stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
	if err != nil {
		t.Fatalf("storeWithConflictCheck: %v", err)
	}
	if deduped || stored.ID != "new-1" {
		t.Fatalf("contradiction must store the new winner, deduped=%v", deduped)
	}

	// The loser is superseded and its validity window is closed.
	oldRow, err := database.GetMemory("old-1")
	if err != nil {
		t.Fatalf("load old: %v", err)
	}
	if oldRow.SupersededBy != "new-1" {
		t.Errorf("old fact must be superseded by new-1, got %q", oldRow.SupersededBy)
	}
	if oldRow.ValidTo == nil || !oldRow.ValidTo.After(now.Add(-2*time.Hour)) {
		t.Errorf("valid_to must be closed near the resolution time, got %v", oldRow.ValidTo)
	}

	// The resolution is recorded with both actors and the deciding rule.
	events, err := database.GetAuditLogs(conflict.ActionSupersede, 10)
	if err != nil {
		t.Fatalf("GetAuditLogs: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected one supersede event, got %d", len(events))
	}
	if events[0].MemoryID != "old-1" || events[0].Actor != "writer-a" {
		t.Errorf("event attribution wrong: %+v", events[0])
	}
	var detail struct {
		WinnerID    string  `json:"winner_id"`
		WinnerActor string  `json:"winner_actor"`
		LoserActor  string  `json:"loser_actor"`
		Rule        string  `json:"rule"`
		Similarity  float32 `json:"similarity"`
	}
	if err := json.Unmarshal([]byte(events[0].Detail), &detail); err != nil {
		t.Fatalf("detail JSON: %v", err)
	}
	if detail.WinnerID != "new-1" || detail.WinnerActor != "writer-a" || detail.LoserActor != "actor-old-1" || detail.Rule != "recency" {
		t.Errorf("detail must name both memories, both actors and the rule: %+v", detail)
	}

	// exclude_superseded retrieval omits the loser; without the flag both
	// are returned with the supersession visible.
	base := bandVecBase()
	excluded, err := database.SearchMemoriesFilteredWithTrust(base, conflictTestSource, "global", 10, "", db.TrustFilter{ExcludeSuperseded: true}, db.PolicyFilter{}, db.TimeWindow{}, "")
	if err != nil {
		t.Fatalf("search with exclude_superseded: %v", err)
	}
	for _, r := range excluded {
		if r.Memory.ID == "old-1" {
			t.Fatal("superseded fact must be omitted with exclude_superseded")
		}
	}
	included, err := database.SearchMemoriesFilteredWithTrust(base, conflictTestSource, "global", 10, "", db.TrustFilter{}, db.PolicyFilter{}, db.TimeWindow{}, "")
	if err != nil {
		t.Fatalf("search without filter: %v", err)
	}
	foundLoser := false
	for _, r := range included {
		if r.Memory.ID == "old-1" {
			foundLoser = true
			if r.Memory.SupersededBy != "new-1" {
				t.Errorf("supersession must be visible in the payload, got %q", r.Memory.SupersededBy)
			}
		}
	}
	if !foundLoser {
		t.Error("superseded fact must be returned without exclude_superseded")
	}
}

func TestStoreWithConflictCheckImportanceOldWins(t *testing.T) {
	database := helperMemDB(t)
	now := time.Now().UTC()
	// The curated old fact (importance 0.9) outranks the flaky new write.
	saveEngineered(t, database, "old-1", "the daemon listens on port 8787", bandVec(t, 0.90), 0.9, now.Add(-time.Hour))

	attr := Attribution{Author: "writer-a", SessionID: "sess-a"}
	m := &db.Memory{
		ID:              "new-1",
		Content:         "the daemon runs on port 9000",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       bandVecBase(),
		EmbeddingSource: conflictTestSource,
		CreatedBy:       attr.Author,
		CreatedSession:  attr.SessionID,
		Importance:      0.3,
	}

	checker := conflict.NewChecker(database, conflictCfg())
	checker.SetVerdictProvider(portFakeProvider{})
	stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
	if err != nil {
		t.Fatalf("storeWithConflictCheck: %v", err)
	}
	if deduped {
		t.Fatal("a contradiction must not be deduplicated")
	}
	// The new write is saved but pre-marked as superseded by the winner.
	newRow, err := database.GetMemory(stored.ID)
	if err != nil {
		t.Fatalf("load new: %v", err)
	}
	if newRow.SupersededBy != "old-1" {
		t.Errorf("the flaky new write must be superseded by the curated old fact, got %q", newRow.SupersededBy)
	}
	oldRow, err := database.GetMemory("old-1")
	if err != nil {
		t.Fatalf("load old: %v", err)
	}
	if oldRow.SupersededBy != "" {
		t.Errorf("the winner must not be superseded, got %q", oldRow.SupersededBy)
	}

	events, err := database.GetAuditLogs(conflict.ActionSupersede, 10)
	if err != nil {
		t.Fatalf("GetAuditLogs: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected one supersede event, got %d", len(events))
	}
	var detail struct {
		WinnerID    string `json:"winner_id"`
		WinnerActor string `json:"winner_actor"`
		Rule        string `json:"rule"`
	}
	if err := json.Unmarshal([]byte(events[0].Detail), &detail); err != nil {
		t.Fatalf("detail JSON: %v", err)
	}
	if detail.WinnerID != "old-1" || detail.WinnerActor != "actor-old-1" || detail.Rule != "importance" {
		t.Errorf("detail must name the old winner and the importance rule: %+v", detail)
	}
}

// bandVecBase is the unit base vector used as the "new write" embedding.
func bandVecBase() []float32 {
	v := make([]float32, db.EmbeddingDim)
	v[0] = 1.0
	return v
}

// failingProvider fails every verdict request, simulating an LLM outage
// on the verdict tier.
type failingProvider struct{}

func (failingProvider) Verdicts(_ context.Context, pairs []conflict.Pair) ([]conflict.Verdict, error) {
	return nil, fmt.Errorf("llm unreachable")
}

func TestStoreWithConflictCheckProviderErrorStoresBothAndAudits(t *testing.T) {
	// A failing verdict tier degrades to ambiguous: both rows are stored
	// unchanged and the disagreement is surfaced — a write never fails
	// because the LLM is down (#506).
	database := helperMemDB(t)
	now := time.Now().UTC()
	saveEngineered(t, database, "old-1", "the daemon listens on port 8787", bandVec(t, 0.90), 0.5, now.Add(-time.Hour))

	attr := Attribution{Author: "writer-a", SessionID: "sess-a"}
	m := &db.Memory{
		ID:              "new-1",
		Content:         "the daemon runs on port 9000",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       bandVecBase(),
		EmbeddingSource: conflictTestSource,
		CreatedBy:       attr.Author,
		CreatedSession:  attr.SessionID,
	}

	checker := conflict.NewChecker(database, conflictCfg())
	checker.SetVerdictProvider(failingProvider{})
	stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
	if err != nil {
		t.Fatalf("verdict tier failure must not fail the write: %v", err)
	}
	if deduped || stored.ID != "new-1" {
		t.Fatalf("verdict failure must store the new memory unchanged, deduped=%v", deduped)
	}
	if n := rowCount(t, database); n != 2 {
		t.Fatalf("verdict failure must store both rows, got %d", n)
	}
	oldRow, err := database.GetMemory("old-1")
	if err != nil {
		t.Fatalf("load old: %v", err)
	}
	if oldRow.SupersededBy != "" {
		t.Errorf("verdict failure must not supersede, got superseded_by=%q", oldRow.SupersededBy)
	}

	events, err := database.GetAuditLogs(conflict.ActionConflictPending, 10)
	if err != nil {
		t.Fatalf("GetAuditLogs: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected one conflict_pending event, got %d", len(events))
	}
	if events[0].MemoryID != "new-1" {
		t.Errorf("conflict_pending must reference the new memory, got %+v", events[0])
	}
}

func TestStoreWithConflictCheckCheckerErrorDegradesToStoreBoth(t *testing.T) {
	// A checker failure (candidate recall error) degrades to the legacy
	// store-both behavior: the memory is stored unchanged and the write
	// never fails (#506). The recall error is provoked deterministically
	// by corrupting one stored row's embedding column (invalid JSON), so
	// candidate hydration fails while the write path keeps working.
	database := helperMemDB(t)
	now := time.Now().UTC()
	saveEngineered(t, database, "old-1", "the daemon listens on port 8787", bandVec(t, 0.90), 0.5, now.Add(-time.Hour))
	if _, err := database.Conn().Exec("UPDATE memories SET embedding = 'not-json' WHERE id = 'old-1'"); err != nil {
		t.Fatalf("corrupt embedding: %v", err)
	}

	attr := Attribution{Author: "writer-a", SessionID: "sess-a"}
	m := &db.Memory{
		ID:              "new-1",
		Content:         "the daemon runs on port 9000",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       bandVecBase(),
		EmbeddingSource: conflictTestSource,
		CreatedBy:       attr.Author,
		CreatedSession:  attr.SessionID,
	}

	checker := conflict.NewChecker(database, conflictCfg())
	stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
	if err != nil {
		t.Fatalf("checker failure must not fail the write: %v", err)
	}
	if deduped || stored.ID != "new-1" {
		t.Fatalf("checker failure must store the new memory unchanged, deduped=%v", deduped)
	}
	if n := rowCount(t, database); n != 2 {
		t.Fatalf("checker failure must store both rows, got %d", n)
	}
	// The corrupted row cannot be hydrated through GetMemory (its
	// embedding is unreadable), so the supersession check reads the raw
	// column instead.
	var sup sql.NullString
	if err := database.Conn().QueryRow("SELECT superseded_by FROM memories WHERE id = 'old-1'").Scan(&sup); err != nil {
		t.Fatalf("load superseded_by: %v", err)
	}
	if sup.String != "" {
		t.Errorf("checker failure must not supersede, got superseded_by=%q", sup.String)
	}
}
