package db

import "testing"

func seedCandidateEntities(t *testing.T, database *DB) {
	t.Helper()
	entities := []*Entity{
		{ID: "cand-alice", Name: "Alice", Type: "person", Aliases: []string{"Ali"}},
		{ID: "cand-alicechar", Name: "Alice Charón", Type: "person", Aliases: []string{}},
		{ID: "cand-projectx", Name: "ProjectX", Type: "project", Aliases: []string{"Alice"}},
	}
	for _, e := range entities {
		if err := database.SaveEntity(e); err != nil {
			t.Fatalf("seed entity %s: %v", e.ID, err)
		}
	}
}

func TestResolveEntityCandidates_ExactNameOutranksAliasAndNormalized(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("Alice", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) == 0 {
		t.Fatal("expected at least one candidate")
	}
	if got[0].EntityID != "cand-alice" || got[0].MatchKind != "exact_name" {
		t.Fatalf("expected exact_name match for cand-alice first, got %+v", got[0])
	}
	if got[0].Score != 1.0 {
		t.Errorf("expected score 1.0, got %v", got[0].Score)
	}
}

func TestResolveEntityCandidates_AmbiguousReturnsMultiple(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	// "Alice" exactly matches cand-alice's name and ProjectX's alias —
	// two distinct entities, both legitimately named as candidates.
	got, err := database.ResolveEntityCandidates("Alice", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) < 2 {
		t.Fatalf("expected multiple ambiguous candidates, got %d: %+v", len(got), got)
	}
	foundAlice, foundProjectX := false, false
	for _, c := range got {
		if c.EntityID == "cand-alice" {
			foundAlice = true
		}
		if c.EntityID == "cand-projectx" {
			foundProjectX = true
		}
	}
	if !foundAlice || !foundProjectX {
		t.Fatalf("expected both cand-alice (name) and cand-projectx (alias) in results, got %+v", got)
	}
}

func TestResolveEntityCandidates_TypeFilterIsStrict(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("Alice", "project", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	for _, c := range got {
		if c.Type != "project" {
			t.Fatalf("expected only project-type candidates, got %+v", c)
		}
	}
	if len(got) != 1 || got[0].EntityID != "cand-projectx" {
		t.Fatalf("expected exactly cand-projectx, got %+v", got)
	}
}

func TestResolveEntityCandidates_NormalizedMatchAcrossDiacritics(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("alice charon", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	found := false
	for _, c := range got {
		if c.EntityID == "cand-alicechar" && c.MatchKind == "normalized_name" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected normalized_name match for cand-alicechar, got %+v", got)
	}
}

func TestResolveEntityCandidates_AliasHintExpandsMatch(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("Nobody Named This", "", []string{"Ali"}, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) != 1 || got[0].EntityID != "cand-alice" || got[0].MatchKind != "exact_alias" {
		t.Fatalf("expected exact_alias match via hint, got %+v", got)
	}
}

func TestResolveEntityCandidates_RejectsEmailAliasHint(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	// The email hint would otherwise not match anything anyway, but the real
	// assertion is that a hint shaped like PII never reaches comparison —
	// verified indirectly by ensuring no error occurs and no bogus stored
	// state was created.
	got, err := database.ResolveEntityCandidates("Nobody Named This", "", []string{"alice@example.com"}, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("expected email hint to be dropped and produce no match, got %+v", got)
	}

	all, err := database.ListEntities()
	if err != nil {
		t.Fatalf("ListEntities: %v", err)
	}
	if len(all) != 3 {
		t.Fatalf("expected entity count unchanged at 3, got %d", len(all))
	}
}

func TestResolveEntityCandidates_LimitCapsResults(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("Alice", "", nil, 1)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("expected exactly 1 candidate with limit=1, got %d", len(got))
	}
}

func TestResolveEntityCandidates_NoMatchReturnsEmpty(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	got, err := database.ResolveEntityCandidates("Completely Unrelated Query", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("expected no candidates, got %+v", got)
	}
}

func TestResolveEntityCandidates_EmptyQueryErrors(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	if _, err := database.ResolveEntityCandidates("   ", "", nil, 10); err == nil {
		t.Fatal("expected an error for an empty/whitespace-only query")
	}
}

func TestResolveEntityCandidates_OversizedQueryErrors(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	huge := make([]byte, 600)
	for i := range huge {
		huge[i] = 'a'
	}
	if _, err := database.ResolveEntityCandidates(string(huge), "", nil, 10); err == nil {
		t.Fatal("expected an error for an oversized query")
	}
}

func TestResolveEntityCandidates_StableOrderingIsDeterministic(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	first, err := database.ResolveEntityCandidates("Alice", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	second, err := database.ResolveEntityCandidates("Alice", "", nil, 10)
	if err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}
	if len(first) != len(second) {
		t.Fatalf("nondeterministic result length: %d vs %d", len(first), len(second))
	}
	for i := range first {
		if first[i].EntityID != second[i].EntityID {
			t.Fatalf("nondeterministic ordering at index %d: %s vs %s", i, first[i].EntityID, second[i].EntityID)
		}
	}
}

func TestResolveEntityCandidates_NeverMutatesEntities(t *testing.T) {
	database := newTestDB(t)
	seedCandidateEntities(t, database)

	before, err := database.GetEntityByID("cand-alice")
	if err != nil {
		t.Fatalf("GetEntityByID: %v", err)
	}

	if _, err := database.ResolveEntityCandidates("Alice Charhon", "", []string{"Ali", "someone@example.com"}, 10); err != nil {
		t.Fatalf("ResolveEntityCandidates: %v", err)
	}

	after, err := database.GetEntityByID("cand-alice")
	if err != nil {
		t.Fatalf("GetEntityByID: %v", err)
	}
	if !before.UpdatedAt.Equal(after.UpdatedAt) {
		t.Fatalf("expected UpdatedAt unchanged, before=%v after=%v", before.UpdatedAt, after.UpdatedAt)
	}
	if len(after.Aliases) != len(before.Aliases) {
		t.Fatalf("expected aliases unchanged, before=%v after=%v", before.Aliases, after.Aliases)
	}
}
