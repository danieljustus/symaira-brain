package db

import (
	"testing"
	"time"
)

// ---------------------------------------------------------------------------
// Semantic kinds (#486)
// ---------------------------------------------------------------------------

func TestNormalizeKindCanonicalAndSynonyms(t *testing.T) {
	cases := map[string]string{
		"user":                   KindUser,
		"User":                   KindUser,
		"preference":             KindUser,
		"preferences":            KindUser,
		"personal":               KindUser,
		"feedback":               KindFeedback,
		"correction":             KindFeedback,
		"critique":               KindFeedback,
		"project":                KindProject,
		"rule":                   KindProject,
		"constraints":            KindProject,
		"architectural-decision": KindProject,
		"reference":              KindReference,
		"documentation":          KindReference,
		"how-to":                 KindReference,
	}
	for input, want := range cases {
		got, ok := NormalizeKind(input)
		if !ok {
			t.Errorf("NormalizeKind(%q) = not ok, want %q", input, want)
			continue
		}
		if got != want {
			t.Errorf("NormalizeKind(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestNormalizeKindRejectsUnknown(t *testing.T) {
	for _, input := range []string{"", "banana", "user-preference-meta", "???", " "} {
		if got, ok := NormalizeKind(input); ok {
			t.Errorf("NormalizeKind(%q) = %q, want rejection", input, got)
		}
	}
}

func TestKindRankOrdersIdentityFirst(t *testing.T) {
	if KindRank(KindUser) >= KindRank(KindFeedback) {
		t.Error("user must rank before feedback")
	}
	if KindRank(KindFeedback) >= KindRank(KindProject) {
		t.Error("feedback must rank before project")
	}
	if KindRank(KindProject) >= KindRank(KindReference) {
		t.Error("project must rank before reference")
	}
	if KindRank("") <= KindRank(KindReference) {
		t.Error("unclassified must rank last")
	}
}

// ---------------------------------------------------------------------------
// Staging and review flow (#485)
// ---------------------------------------------------------------------------

func TestStagedMemoryExcludedFromListAndSearchUntilPromoted(t *testing.T) {
	db := newTestDB(t)

	live := &Memory{
		ID:        "live-1",
		Content:   "user prefers dark mode",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: embeddingVector(1.0, 0.0, 0.0),
		Kind:      KindUser,
	}
	staged := &Memory{
		ID:           "staged-1",
		Content:      "user prefers dark mode (autonomous guess)",
		Scope:        "global",
		Metadata:     map[string]string{},
		Embedding:    embeddingVector(1.0, 0.0, 0.0),
		Kind:         KindUser,
		ReviewStatus: ReviewStaged,
	}
	if err := db.SaveMemory(live); err != nil {
		t.Fatal(err)
	}
	if err := db.SaveMemory(staged); err != nil {
		t.Fatal(err)
	}

	// List excludes staged.
	mems, err := db.ListMemories("", 0, 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(mems) != 1 || mems[0].ID != "live-1" {
		t.Fatalf("ListMemories = %d rows, want only the live memory (got %v)", len(mems), idsOf(mems))
	}

	// Search excludes staged (empty embedding source matches unset source).
	results, err := db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 10, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 1 || results[0].Memory.ID != "live-1" {
		t.Fatalf("SearchMemories = %d results, want only the live memory", len(results))
	}

	// Review queue sees the candidate.
	cands, err := db.ListStagedMemories(100)
	if err != nil {
		t.Fatal(err)
	}
	if len(cands) != 1 || cands[0].ID != "staged-1" {
		t.Fatalf("ListStagedMemories = %v, want staged-1", idsOf(cands))
	}

	// Promote → retrievable.
	if err := db.SetMemoryReviewStatus("staged-1", ReviewApproved); err != nil {
		t.Fatal(err)
	}
	results, err = db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 10, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 2 {
		t.Fatalf("after promote: SearchMemories = %d results, want 2", len(results))
	}
}

func TestReviewStatusValidation(t *testing.T) {
	db := newTestDB(t)
	m := &Memory{ID: "v-1", Content: "x", Scope: "global", Metadata: map[string]string{}, Embedding: embeddingVector(1.0)}
	if err := db.SaveMemory(m); err != nil {
		t.Fatal(err)
	}
	if err := db.SetMemoryReviewStatus("v-1", "bogus"); err == nil {
		t.Fatal("SetMemoryReviewStatus must reject unknown statuses")
	}
	if err := db.SetMemoryReviewStatus("missing-id", ReviewStaged); err == nil {
		t.Fatal("SetMemoryReviewStatus must fail for unknown ids")
	}
}

func TestSetMemoryKindValidatesAndPersists(t *testing.T) {
	db := newTestDB(t)
	m := &Memory{ID: "k-1", Content: "x", Scope: "global", Metadata: map[string]string{}, Embedding: embeddingVector(1.0)}
	if err := db.SaveMemory(m); err != nil {
		t.Fatal(err)
	}
	if err := db.SetMemoryKind("k-1", "preferences"); err != nil {
		t.Fatalf("SetMemoryKind with synonym failed: %v", err)
	}
	got, err := db.GetMemory("k-1")
	if err != nil {
		t.Fatal(err)
	}
	if got.Kind != KindUser {
		t.Fatalf("kind after synonym set = %q, want %q", got.Kind, KindUser)
	}
	if err := db.SetMemoryKind("k-1", "banana"); err == nil {
		t.Fatal("SetMemoryKind must reject unknown kinds")
	}
}

// ---------------------------------------------------------------------------
// Aging: decay factor, retirement (#491)
// ---------------------------------------------------------------------------

func TestDecayFactorRoundTripAndSearchMultiplier(t *testing.T) {
	db := newTestDB(t)

	m := &Memory{
		ID:        "d-1",
		Content:   "stale fact",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: embeddingVector(1.0, 0.0, 0.0),
		CreatedAt: time.Now().UTC().Add(-500 * 24 * time.Hour),
	}
	if err := db.SaveMemory(m); err != nil {
		t.Fatal(err)
	}
	if err := db.SetDecayFactor("d-1", 0.2); err != nil {
		t.Fatal(err)
	}
	got, err := db.GetMemory("d-1")
	if err != nil {
		t.Fatal(err)
	}
	if got.DecayFactor != 0.2 {
		t.Fatalf("decay factor = %v, want 0.2", got.DecayFactor)
	}

	results, err := db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 10, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 1 {
		t.Fatalf("search = %d results, want 1", len(results))
	}
	// Decay multiplies the composite score: same relevance/recency as a
	// fresh memory, but 0.2× on top.
	plain := CompositeScore(1.0, got.CreatedAt, 0.05, 1, got.LastAccess, nil, DefaultRankingWeights())
	if results[0].Score > float32(plain*0.2+1e-6) {
		t.Fatalf("decayed score %v not multiplied by decay factor (plain %v)", results[0].Score, plain*0.2)
	}
}

func TestRetireFlagsAndHides(t *testing.T) {
	db := newTestDB(t)
	m := &Memory{ID: "r-1", Content: "retired fact", Scope: "global", Metadata: map[string]string{}, Embedding: embeddingVector(1.0, 0.0, 0.0)}
	if err := db.SaveMemory(m); err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	if err := db.RetireMemory("r-1", now); err != nil {
		t.Fatal(err)
	}

	// Hidden from list and search.
	mems, err := db.ListMemories("", 0, 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(mems) != 0 {
		t.Fatalf("retired memory still listed: %v", idsOf(mems))
	}
	results, err := db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 10, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 0 {
		t.Fatalf("retired memory still searchable: %v", results)
	}

	// Flag, not delete: row survives with the timestamp.
	got, err := db.GetMemory("r-1")
	if err != nil {
		t.Fatal(err)
	}
	if got.RetiredAt == nil {
		t.Fatal("retired_at not set")
	}

	// Unretire restores retrievability.
	if err := db.UnretireMemory("r-1"); err != nil {
		t.Fatal(err)
	}
	results, err = db.SearchMemoriesFiltered(embeddingVector(1.0, 0.0, 0.0), "", "", 10, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 1 {
		t.Fatalf("after unretire: search = %d results, want 1", len(results))
	}
}

func TestAgingCandidatesExcludesRetired(t *testing.T) {
	db := newTestDB(t)
	live := &Memory{ID: "a-live", Content: "live", Scope: "global", Metadata: map[string]string{}}
	retired := &Memory{ID: "a-ret", Content: "retired", Scope: "global", Metadata: map[string]string{}}
	if err := db.SaveMemory(live); err != nil {
		t.Fatal(err)
	}
	if err := db.SaveMemory(retired); err != nil {
		t.Fatal(err)
	}
	if err := db.RetireMemory("a-ret", time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	cands, err := db.AgingCandidates()
	if err != nil {
		t.Fatal(err)
	}
	if len(cands) != 1 || cands[0].ID != "a-live" {
		t.Fatalf("AgingCandidates = %v, want only live", idsOf(cands))
	}
}

func idsOf(mems []*Memory) []string {
	ids := make([]string, len(mems))
	for i, m := range mems {
		ids[i] = m.ID
	}
	return ids
}
