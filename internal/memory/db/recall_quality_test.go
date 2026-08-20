package db

import (
	"math"
	"testing"
	"time"
)

// ---------------------------------------------------------------------------
// Memory-to-memory associations (#488)
// ---------------------------------------------------------------------------

func TestSaveMemoryAssociationRoundTrip(t *testing.T) {
	db := newTestDB(t)
	if err := db.SaveMemoryAssociation("a", "b", 0.7, "tester"); err != nil {
		t.Fatal(err)
	}
	got, err := db.AssociationsFrom([]string{"a"})
	if err != nil {
		t.Fatal(err)
	}
	if got["a"]["b"] != 0.7 {
		t.Fatalf("edge weight = %v, want 0.7", got["a"]["b"])
	}

	// Upsert keeps the maximum weight.
	if err := db.SaveMemoryAssociation("a", "b", 0.4, "tester"); err != nil {
		t.Fatal(err)
	}
	got, _ = db.AssociationsFrom([]string{"a"})
	if got["a"]["b"] != 0.7 {
		t.Fatalf("max-weight upsert failed: %v", got["a"]["b"])
	}
	if err := db.SaveMemoryAssociation("a", "b", 0.9, "tester"); err != nil {
		t.Fatal(err)
	}
	got, _ = db.AssociationsFrom([]string{"a"})
	if got["a"]["b"] != 0.9 {
		t.Fatalf("max-weight upsert failed (raise): %v", got["a"]["b"])
	}

	if err := db.SaveMemoryAssociation("a", "a", 0.5, "tester"); err == nil {
		t.Fatal("self-edge must be refused")
	}
	if err := db.SaveMemoryAssociation("a", "b", 0, "tester"); err == nil {
		t.Fatal("zero weight must be refused")
	}
}

func TestSeedAssociationsCoRetrieval(t *testing.T) {
	db := newTestDB(t)
	m1 := &Memory{ID: "m1", Content: "one", Scope: "global", Metadata: map[string]string{}}
	m2 := &Memory{ID: "m2", Content: "two", Scope: "global", Metadata: map[string]string{}}
	m3 := &Memory{ID: "m3", Content: "three", Scope: "global", Metadata: map[string]string{}}
	for _, m := range []*Memory{m1, m2, m3} {
		if err := db.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}
	qid, err := db.LogQuery("test", "global", "sess", "memory_search", "query", "", 1)
	if err != nil {
		t.Fatal(err)
	}
	if err := db.RecordQueryResults(qid, []QueryResultRef{
		{MemoryID: "m1", Rank: 0, Score: 0.9},
		{MemoryID: "m2", Rank: 1, Score: 0.8},
		{MemoryID: "m3", Rank: 2, Score: 0.7},
	}); err != nil {
		t.Fatal(err)
	}

	inserted, err := db.SeedMemoryAssociations("test")
	if err != nil {
		t.Fatal(err)
	}
	// 3 choose 2 = 3 edges.
	if inserted != 3 {
		t.Fatalf("seeded %d edges, want 3", inserted)
	}
	edges, err := db.AssociationsFrom([]string{"m1", "m2"})
	if err != nil {
		t.Fatal(err)
	}
	if edges["m1"]["m2"] != CoRetrievalWeight {
		t.Fatalf("co-retrieval edge weight = %v, want %v", edges["m1"]["m2"], CoRetrievalWeight)
	}
	if edges["m1"]["m3"] != CoRetrievalWeight {
		t.Fatalf("co-retrieval edge m1→m3 missing: %v", edges["m1"])
	}
	if edges["m2"]["m3"] != CoRetrievalWeight {
		t.Fatalf("co-retrieval edge m2→m3 missing: %v", edges["m2"])
	}
}

func TestSeedAssociationsSharedEntity(t *testing.T) {
	db := newTestDB(t)
	m1 := &Memory{ID: "m1", Content: "one", Scope: "global", Metadata: map[string]string{}}
	m2 := &Memory{ID: "m2", Content: "two", Scope: "global", Metadata: map[string]string{}}
	for _, m := range []*Memory{m1, m2} {
		if err := db.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}
	entity := &Entity{ID: "e1", Name: "Irene", Type: "person", Aliases: []string{}, CreatedBy: "test"}
	if err := db.SaveEntity(entity); err != nil {
		t.Fatal(err)
	}
	if err := db.LinkMemoryToEntity("m1", "e1"); err != nil {
		t.Fatal(err)
	}
	if err := db.LinkMemoryToEntity("m2", "e1"); err != nil {
		t.Fatal(err)
	}

	inserted, err := db.SeedMemoryAssociations("test")
	if err != nil {
		t.Fatal(err)
	}
	if inserted != 1 {
		t.Fatalf("seeded %d edges, want 1 (shared entity)", inserted)
	}
	edges, _ := db.AssociationsFrom([]string{"m1", "m2"})
	if edges["m1"]["m2"] != SharedEntityWeight && edges["m2"]["m1"] != SharedEntityWeight {
		t.Fatalf("shared-entity edge weight = %v/%v, want %v in one direction",
			edges["m1"]["m2"], edges["m2"]["m1"], SharedEntityWeight)
	}
}

func TestSeedAssociationsConsolidationSiblings(t *testing.T) {
	db := newTestDB(t)
	// The consolidation parent must exist: consolidated_into_id carries a
	// foreign key to memories(id).
	parent := &Memory{ID: "parent-1", Content: "merged fact", Scope: "global", Metadata: map[string]string{}}
	if err := db.SaveMemory(parent); err != nil {
		t.Fatal(err)
	}
	m1 := &Memory{ID: "m1", Content: "one", Scope: "global", Metadata: map[string]string{}}
	m2 := &Memory{ID: "m2", Content: "two", Scope: "global", Metadata: map[string]string{}}
	for _, m := range []*Memory{m1, m2} {
		if err := db.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}
	tx, err := db.BeginTransaction()
	if err != nil {
		t.Fatal(err)
	}
	if err := db.UpdateMemoryStatusTx(tx, "m1", "consolidated", "parent-1"); err != nil {
		t.Fatal(err)
	}
	if err := db.UpdateMemoryStatusTx(tx, "m2", "consolidated", "parent-1"); err != nil {
		t.Fatal(err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}

	inserted, err := db.SeedMemoryAssociations("test")
	if err != nil {
		t.Fatal(err)
	}
	if inserted != 1 {
		t.Fatalf("seeded %d edges, want 1 (consolidation siblings)", inserted)
	}
	edges, _ := db.AssociationsFrom([]string{"m1"})
	if edges["m1"]["m2"] != ConsolidationWeight {
		t.Fatalf("consolidation edge weight = %v, want %v", edges["m1"]["m2"], ConsolidationWeight)
	}
}

func TestSpreadingBonusHopMath(t *testing.T) {
	db := newTestDB(t)
	// a → b (0.8), b → c (0.5): seed a with score 1.0.
	for _, edge := range [][3]interface{}{
		{"a", "b", 0.8},
		{"b", "c", 0.5},
		{"a", "x", 1.0}, // stronger 1-hop path to a dead end
	} {
		if err := db.SaveMemoryAssociation(edge[0].(string), edge[1].(string), edge[2].(float64), "test"); err != nil {
			t.Fatal(err)
		}
	}
	bonus, err := db.SpreadingBonus(map[string]float64{"a": 1.0}, 32, 32)
	if err != nil {
		t.Fatal(err)
	}
	// b: 1.0 × 0.8 × 0.5 = 0.4
	if math.Abs(bonus["b"]-0.4) > 1e-9 {
		t.Fatalf("hop-1 bonus for b = %v, want 0.4", bonus["b"])
	}
	// c via b: 0.4 × 0.5 × 0.5 = 0.1
	if math.Abs(bonus["c"]-0.1) > 1e-9 {
		t.Fatalf("hop-2 bonus for c = %v, want 0.1", bonus["c"])
	}
	// x: 1.0 × 1.0 × 0.5 = 0.5 (max over paths)
	if math.Abs(bonus["x"]-0.5) > 1e-9 {
		t.Fatalf("hop-1 bonus for x = %v, want 0.5", bonus["x"])
	}
}

func TestSpreadingBonusSurfacesAssociatedMemoryInSearch(t *testing.T) {
	db := newTestDB(t)

	// Two memories: a strong hit and an associated fact that is otherwise
	// irrelevant (different embedding space).
	hit := &Memory{ID: "hit", Content: "the api uses port 8080", Scope: "global", Metadata: map[string]string{}, Embedding: embeddingVector(1.0, 0.0, 0.0)}
	assoc := &Memory{ID: "assoc", Content: "the api uses port 8080 (related note)", Scope: "global", Metadata: map[string]string{}, Embedding: embeddingVector(0.0, 1.0, 0.0)}
	for _, m := range []*Memory{hit, assoc} {
		if err := db.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}
	if err := db.SaveMemoryAssociation("hit", "assoc", 1.0, "test"); err != nil {
		t.Fatal(err)
	}

	// Default weights: spreading disabled → only the hit is returned.
	results, err := db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 5, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 1 || results[0].Memory.ID != "hit" {
		t.Fatalf("without spreading: %d results, want only hit", len(results))
	}

	// Spreading enabled: the associated memory surfaces.
	w := DefaultRankingWeights()
	w.SpreadingWeight = 1.0
	results, err = db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 5, "", w)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, r := range results {
		if r.Memory.ID == "assoc" {
			found = true
		}
	}
	if !found {
		t.Fatalf("associated memory did not surface with spreading enabled: %+v", results)
	}
}

// ---------------------------------------------------------------------------
// Spacing-aware access reinforcement (#489)
// ---------------------------------------------------------------------------

func TestTrackMemoryAccessShiftsPrevAccess(t *testing.T) {
	db := newTestDB(t)
	m := &Memory{ID: "t1", Content: "x", Scope: "global", Metadata: map[string]string{}}
	if err := db.SaveMemory(m); err != nil {
		t.Fatal(err)
	}

	first := time.Now().UTC().Add(-10 * time.Minute)
	if _, err := db.conn.Exec("UPDATE memories SET last_access = ? WHERE id = ?", first, "t1"); err != nil {
		t.Fatal(err)
	}
	if err := db.TrackMemoryAccess("t1"); err != nil {
		t.Fatal(err)
	}
	got, err := db.GetMemory("t1")
	if err != nil {
		t.Fatal(err)
	}
	if got.PrevAccess == nil || !got.PrevAccess.Equal(first) {
		t.Fatalf("prev_access = %v, want %v", got.PrevAccess, first)
	}
	if got.LastAccess == nil || got.LastAccess.Before(first) {
		t.Fatalf("last_access = %v, want after %v", got.LastAccess, first)
	}
	if got.AccessCount != 2 {
		t.Fatalf("access_count = %d, want 2", got.AccessCount)
	}
}

func TestCompositeScoreSpacingAware(t *testing.T) {
	now := time.Now().UTC()
	created := now.AddDate(0, 0, -10)
	weights := DefaultRankingWeights()
	weights.AccessReinforcementWeight = 1.0
	weights.AccessSpacingHalfLife = 30

	// 20 accesses crammed into one session: gap ≈ 0 → boost collapses.
	burstLast := now.Add(-30 * time.Minute)
	burstPrev := now.Add(-60 * time.Minute)
	burst := CompositeScore(0.8, created, 0.5, 20, &burstLast, &burstPrev, weights)

	// 20 accesses spread over months: long gaps → full count boost.
	spreadLast := now.Add(-2 * time.Hour)
	spreadPrev := now.AddDate(0, 0, -60)
	spread := CompositeScore(0.8, created, 0.5, 20, &spreadLast, &spreadPrev, weights)

	if spread <= burst {
		t.Fatalf("spread reinforcement %v must exceed burst reinforcement %v", spread, burst)
	}

	// Legacy row without prev_access keeps the count-based behaviour:
	// the full count boost applies, which must exceed the burst case.
	legacy := CompositeScore(0.8, created, 0.5, 20, &burstLast, nil, weights)
	if burst >= legacy {
		t.Fatalf("burst %v must be below legacy %v", burst, legacy)
	}
}
