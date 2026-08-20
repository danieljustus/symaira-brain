package db

import "testing"

// TestSearchMemoriesWithProfile_SinglePassProvenance verifies that a
// multi-scope profile search returns memories from every scope with the
// SourceScope tagged from the hydrated row (not a loop variable).
//
// Regression test for #535.
func TestSearchMemoriesWithProfile_SinglePassProvenance(t *testing.T) {
	db := newTestDB(t)

	cp := &ContextProfile{ID: "p535", Name: "p535-test", BaseScope: "global"}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatal(err)
	}
	if err := db.AddContextProfileLink(&ContextProfileLink{
		ProfileID: "p535", Scope: "project", PrecedenceOrder: 1,
	}); err != nil {
		t.Fatal(err)
	}

	// Same embedding (one LSH bucket) so both are candidates in one pass;
	// only the scope differs.
	globalMem := &Memory{ID: "g1", Content: "global memory", Scope: "global", Embedding: oneHotVec(0), Metadata: map[string]string{}}
	projectMem := &Memory{ID: "p1", Content: "project memory", Scope: "project", Embedding: oneHotVec(0), Metadata: map[string]string{}}
	if err := db.SaveMemory(globalMem); err != nil {
		t.Fatal(err)
	}
	if err := db.SaveMemory(projectMem); err != nil {
		t.Fatal(err)
	}

	results, err := db.SearchMemoriesWithProfile(oneHotVec(0), "", "p535-test", 10, "", TrustFilter{}, PolicyFilter{}, TimeWindow{})
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 2 {
		t.Fatalf("expected 2 results (global + project), got %d", len(results))
	}

	byID := map[string]string{}
	for _, r := range results {
		if r.SourceProfile != "p535-test" {
			t.Errorf("result %s: SourceProfile=%q, want %q", r.Memory.ID, r.SourceProfile, "p535-test")
		}
		byID[r.Memory.ID] = r.SourceScope
	}
	if byID["g1"] != "global" {
		t.Errorf("g1 SourceScope=%q, want %q", byID["g1"], "global")
	}
	if byID["p1"] != "project" {
		t.Errorf("p1 SourceScope=%q, want %q", byID["p1"], "project")
	}
}

// TestSearchMemoriesWithProfile_AccessTrackingFinalOnly verifies that only
// the memories in the final returned set get their access recorded: exactly
// one of three equal candidates is returned for limit=1, and only that memory
// sees its access_count increment.
//
// Regression test for #535.
func TestSearchMemoriesWithProfile_AccessTrackingFinalOnly(t *testing.T) {
	db := newTestDB(t)

	cp := &ContextProfile{ID: "p535a", Name: "p535a-test", BaseScope: "global"}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatal(err)
	}

	// Three identical embeddings: all land in the same LSH bucket, so all are
	// candidates and get scored; only one makes the final limit.
	ids := []string{"m1", "m2", "m3"}
	for _, id := range ids {
		m := &Memory{ID: id, Content: "mem " + id, Scope: "global", Embedding: oneHotVec(0), Metadata: map[string]string{}}
		if err := db.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}
	before := map[string]int64{}
	for _, id := range ids {
		before[id] = readAccessCount(db, id)
	}

	results, err := db.SearchMemoriesWithProfile(oneHotVec(0), "", "p535a-test", 1, "", TrustFilter{}, PolicyFilter{}, TimeWindow{})
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 1 {
		t.Fatalf("expected exactly 1 result, got %d", len(results))
	}

	incremented := 0
	for _, id := range ids {
		delta := readAccessCount(db, id) - before[id]
		switch {
		case delta == 1:
			incremented++
		case delta > 1:
			t.Errorf("%s access_count incremented by %d, want at most 1", id, delta)
		case delta < 0:
			t.Errorf("%s access_count decreased by %d", id, -delta)
		}
	}
	if incremented != 1 {
		t.Errorf("expected exactly 1 memory incremented, got %d (want: only the returned set)", incremented)
	}
}

func readAccessCount(db *DB, id string) int64 {
	var n int64
	if err := db.conn.QueryRow("SELECT access_count FROM memories WHERE id = ?", id).Scan(&n); err != nil {
		return -1
	}
	return n
}
