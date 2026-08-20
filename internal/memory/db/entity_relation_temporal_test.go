package db

import (
	"testing"
	"time"
)

// TestEntityRelation_TemporalFields_RoundTrip verifies that valid_from and
// valid_until survive a write-read cycle.
func TestEntityRelation_TemporalFields_RoundTrip(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	from := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	until := time.Date(2026, 6, 1, 0, 0, 0, 0, time.UTC)

	rel := &EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		CreatedBy: "tester", ValidFrom: &from, ValidUntil: &until,
	}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("SaveEntityRelation: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected 1 relation, got %d", len(out))
	}
	if out[0].ValidFrom == nil || !out[0].ValidFrom.Equal(from) {
		t.Fatalf("expected valid_from=%v, got %v", from, out[0].ValidFrom)
	}
	if out[0].ValidUntil == nil || !out[0].ValidUntil.Equal(until) {
		t.Fatalf("expected valid_until=%v, got %v", until, out[0].ValidUntil)
	}
}

// TestEntityRelation_TemporalFields_NullDefaults verifies that relations
// created without temporal fields have NULL valid_from and valid_until
// (open-ended, always valid).
func TestEntityRelation_TemporalFields_NullDefaults(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		CreatedBy: "tester",
	}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("SaveEntityRelation: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected 1 relation, got %d", len(out))
	}
	if out[0].ValidFrom != nil {
		t.Fatalf("expected valid_from=nil (open-ended), got %v", out[0].ValidFrom)
	}
	if out[0].ValidUntil != nil {
		t.Fatalf("expected valid_until=nil (open-ended), got %v", out[0].ValidUntil)
	}
}

// TestSaveEntityRelationProvenance_VersionChain_ClosesPreviousOpenInterval
// verifies that creating a newer version of the same (from,to,type) triple
// with a valid_from updates the row in place with new temporal fields.
// The "close" of the previous interval is implicit: the as-of query sees
// the old interval as expired when valid_from moves forward.
func TestSaveEntityRelationProvenance_VersionChain_ClosesPreviousOpenInterval(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	// v1: open-ended (no valid_from/valid_until)
	v1, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "symdesk", SourceRef: "v1", Verification: VerificationUnverified,
	})
	if err != nil {
		t.Fatalf("save v1: %v", err)
	}
	if v1.ValidFrom != nil || v1.ValidUntil != nil {
		t.Fatalf("v1 should be open-ended, got valid_from=%v valid_until=%v", v1.ValidFrom, v1.ValidUntil)
	}

	// v2: new version with valid_from — should update the row in place.
	v2Start := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	v2, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "symdesk", SourceRef: "v2", Verification: VerificationUnverified,
		ValidFrom: &v2Start,
	})
	if err != nil {
		t.Fatalf("save v2: %v", err)
	}
	if v2.ID != v1.ID {
		t.Fatal("v2 should update the existing row in place, not create a new one")
	}
	if v2.ValidFrom == nil || !v2.ValidFrom.Equal(v2Start) {
		t.Fatalf("v2 valid_from should be %v, got %v", v2Start, v2.ValidFrom)
	}
	if v2.SourceRef != "v2" {
		t.Fatalf("v2 SourceRef should be 'v2', got %s", v2.SourceRef)
	}

	// The row should have valid_until nil (v2 is open-ended).
	reloaded, err := database.GetEntityRelationByID(v1.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID(v1): %v", err)
	}
	if reloaded == nil {
		t.Fatal("row should still exist after version chain update")
	}
	if reloaded.ValidUntil != nil {
		t.Fatalf("valid_until should be nil (v2 is open-ended), got %v", reloaded.ValidUntil)
	}

	// Verify as-of behavior: before v2Start the relation should NOT be visible
	// (valid_from=v2Start > before), after v2Start it should be visible.
	before := time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC)
	_, edgesBefore, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &before)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (before): %v", err)
	}
	if len(edgesBefore) != 0 {
		t.Fatalf("expected 0 edges before valid_from, got %d", len(edgesBefore))
	}

	after := time.Date(2026, 4, 1, 0, 0, 0, 0, time.UTC)
	_, edgesAfter, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &after)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (after): %v", err)
	}
	if len(edgesAfter) != 1 {
		t.Fatalf("expected 1 edge after valid_from, got %d", len(edgesAfter))
	}
}

// TestSaveEntityRelationProvenance_VersionChain_ThreeVersions verifies a
// three-version chain: v1 (open) → v2 updates → v3 updates.
func TestSaveEntityRelationProvenance_VersionChain_ThreeVersions(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	v1, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages",
		Source: "cli", SourceRef: "v1",
	})
	if err != nil {
		t.Fatalf("save v1: %v", err)
	}

	t2 := time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC)
	v2, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages",
		Source: "cli", SourceRef: "v2", ValidFrom: &t2,
	})
	if err != nil {
		t.Fatalf("save v2: %v", err)
	}
	if v2.ID != v1.ID {
		t.Fatal("v2 should update the existing row in place")
	}

	t3 := time.Date(2026, 4, 1, 0, 0, 0, 0, time.UTC)
	v3, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages",
		Source: "cli", SourceRef: "v3", ValidFrom: &t3,
	})
	if err != nil {
		t.Fatalf("save v3: %v", err)
	}
	if v3.ID != v1.ID {
		t.Fatal("v3 should also update the existing row in place")
	}

	// After v3: valid_from = t3, valid_until = nil (open)
	r, _ := database.GetEntityRelationByID(v1.ID)
	if r.ValidFrom == nil || !r.ValidFrom.Equal(t3) {
		t.Fatalf("valid_from should be %v (set by v3), got %v", t3, r.ValidFrom)
	}
	if r.ValidUntil != nil {
		t.Fatalf("valid_until should be nil (v3 is open-ended), got %v", r.ValidUntil)
	}
	if r.SourceRef != "v3" {
		t.Fatalf("SourceRef should be 'v3', got %s", r.SourceRef)
	}

	// As-of checks: before t3 the relation is NOT visible (valid_from=t3),
	// after t3 it IS visible.
	before := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	_, edgesBefore, _ := database.GraphNeighborsAsOf(ids["Alice"], 1, &before)
	if len(edgesBefore) != 0 {
		t.Fatalf("expected 0 edges before v3's valid_from, got %d", len(edgesBefore))
	}

	after := time.Date(2026, 5, 1, 0, 0, 0, 0, time.UTC)
	_, edgesAfter, _ := database.GraphNeighborsAsOf(ids["Alice"], 1, &after)
	if len(edgesAfter) != 1 {
		t.Fatalf("expected 1 edge after v3's valid_from, got %d", len(edgesAfter))
	}
}

// TestSaveEntityRelationProvenance_IdempotentRetry_WithTemporalFields
// verifies that retrying the same source+source_ref on a temporal relation
// returns the existing row without mutation.
func TestSaveEntityRelationProvenance_IdempotentRetry_WithTemporalFields(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	from := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	until := time.Date(2026, 12, 31, 0, 0, 0, 0, time.UTC)

	first, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "cli", SourceRef: "idem-1", ValidFrom: &from, ValidUntil: &until,
	})
	if err != nil {
		t.Fatalf("first save: %v", err)
	}

	second, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "cli", SourceRef: "idem-1", ValidFrom: &from, ValidUntil: &until,
	})
	if err != nil {
		t.Fatalf("retry save: %v", err)
	}
	if second.ID != first.ID {
		t.Fatalf("expected idempotent retry, got different IDs: %s vs %s", first.ID, second.ID)
	}
	if second.ValidFrom == nil || !second.ValidFrom.Equal(from) {
		t.Fatalf("expected valid_from preserved, got %v", second.ValidFrom)
	}
}

// TestGraphNeighborsAsOf_IncludesOpenEndedRelations verifies that
// GraphNeighbors with as_of includes relations where valid_from and
// valid_until are both NULL (always valid / open-ended).
func TestGraphNeighborsAsOf_IncludesOpenEndedRelations(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	// Open-ended relation (NULL valid_from / valid_until).
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	asOf := time.Date(2026, 6, 15, 0, 0, 0, 0, time.UTC)
	nodes, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &asOf)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	if len(nodes) != 2 {
		t.Fatalf("expected 2 nodes, got %d", len(nodes))
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge, got %d", len(edges))
	}
}

// TestGraphNeighborsAsOf_ExcludesExpiredRelations verifies that relations
// whose valid_until is before the as_of date are excluded.
func TestGraphNeighborsAsOf_ExcludesExpiredRelations(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob", "Carol")

	// Alice->Bob: expired before 2026-03-01
	expiredUntil := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		ValidUntil: &expiredUntil,
	}); err != nil {
		t.Fatalf("save expired: %v", err)
	}

	// Alice->Carol: valid
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Carol"], RelationType: "manages",
	}); err != nil {
		t.Fatalf("save valid: %v", err)
	}

	asOf := time.Date(2026, 6, 1, 0, 0, 0, 0, time.UTC)
	nodes, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &asOf)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	// Should only reach Carol (not Bob), so 2 nodes (Alice + Carol).
	if len(nodes) != 2 {
		t.Fatalf("expected 2 nodes (expired Bob excluded), got %d: %+v", len(nodes), nodes)
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge (only Alice->Carol), got %d", len(edges))
	}
}

// TestGraphNeighborsAsOf_IncludesActiveRange verifies that relations with
// valid_from <= as_of AND (valid_until IS NULL OR valid_until > as_of) are
// included.
func TestGraphNeighborsAsOf_IncludesActiveRange(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	validFrom := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	validUntil := time.Date(2026, 12, 31, 0, 0, 0, 0, time.UTC)
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		ValidFrom: &validFrom, ValidUntil: &validUntil,
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	// asOf is within the validity window.
	asOf := time.Date(2026, 6, 15, 0, 0, 0, 0, time.UTC)
	_, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &asOf)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge within validity window, got %d", len(edges))
	}

	// asOf is after validity window — should be excluded.
	asOfAfter := time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC)
	_, edgesAfter, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &asOfAfter)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (after): %v", err)
	}
	if len(edgesAfter) != 0 {
		t.Fatalf("expected 0 edges after validity window, got %d", len(edgesAfter))
	}
}

// TestGraphNeighborsAsOf_ExcludesNotYetValid verifies that relations whose
// valid_from is after the as_of date are excluded.
func TestGraphNeighborsAsOf_ExcludesNotYetValid(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	futureFrom := time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC)
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		ValidFrom: &futureFrom,
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	asOf := time.Date(2026, 6, 1, 0, 0, 0, 0, time.UTC)
	nodes, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &asOf)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	// Only Alice (no Bob reached).
	if len(nodes) != 1 {
		t.Fatalf("expected 1 node (not-yet-valid Bob excluded), got %d", len(nodes))
	}
	if len(edges) != 0 {
		t.Fatalf("expected 0 edges, got %d", len(edges))
	}
}

// TestGraphNeighborsAsOf_VersionChainOnlyMostRecentVisible verifies that
// with version chain (update in place), the as_of filter correctly shows
// the relation as active or expired based on the temporal interval.
func TestGraphNeighborsAsOf_VersionChainOnlyMostRecentVisible(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	// v1: open-ended
	v1, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "cli", SourceRef: "v1",
	})
	if err != nil {
		t.Fatalf("save v1: %v", err)
	}
	_ = v1

	// v2: starts 2026-03-01, updates in place
	v2Start := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)
	_, err = database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		Source: "cli", SourceRef: "v2", ValidFrom: &v2Start,
	})
	if err != nil {
		t.Fatalf("save v2: %v", err)
	}

	// At 2026-02-01: valid_from=v2Start which is > before → NOT visible
	before := time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC)
	_, edgesBefore, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &before)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (before): %v", err)
	}
	if len(edgesBefore) != 0 {
		t.Fatalf("expected 0 edges before v2 starts (valid_from=%v), got %d", v2Start, len(edgesBefore))
	}

	// At 2026-04-01: valid_from=v2Start which is <= after, valid_until=nil → visible
	after := time.Date(2026, 4, 1, 0, 0, 0, 0, time.UTC)
	_, edgesAfter, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &after)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf (after): %v", err)
	}
	if len(edgesAfter) != 1 {
		t.Fatalf("expected 1 edge after v2 starts, got %d", len(edgesAfter))
	}
}

// TestParseRelationDate_RFC3339 verifies RFC3339 date parsing.
func TestParseRelationDate_RFC3339(t *testing.T) {
	got, err := ParseRelationDate("2026-06-15T10:30:00Z")
	if err != nil {
		t.Fatalf("ParseRelationDate: %v", err)
	}
	want := time.Date(2026, 6, 15, 10, 30, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("expected %v, got %v", want, got)
	}
}

// TestParseRelationDate_DateOnly verifies YYYY-MM-DD date parsing.
func TestParseRelationDate_DateOnly(t *testing.T) {
	got, err := ParseRelationDate("2026-06-15")
	if err != nil {
		t.Fatalf("ParseRelationDate: %v", err)
	}
	want := time.Date(2026, 6, 15, 0, 0, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("expected %v, got %v", want, got)
	}
}

// TestParseRelationDate_Invalid rejects malformed date strings.
func TestParseRelationDate_Invalid(t *testing.T) {
	invalid := []string{"", "not-a-date", "2026/06/15", "15-06-2026"}
	for _, s := range invalid {
		if _, err := ParseRelationDate(s); err == nil {
			t.Errorf("ParseRelationDate(%q) expected an error, got none", s)
		}
	}
}

// TestGraphNeighborsAsOf_NilAsOfIsNow verifies that passing nil as asOf
// uses the current time (default behavior).
func TestGraphNeighborsAsOf_NilAsOfIsNow(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	// Open-ended relation.
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	// nil asOf → should use now, so the open-ended relation is included.
	_, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, nil)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf(nil): %v", err)
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge with nil as_of (default now), got %d", len(edges))
	}
}

// TestSaveEntityRelationProvenance_VersionChain_DifferentSourceRefEnriches
// verifies that a different source_ref without valid_from enriches the
// existing unverified relation in place (no version chain), preserving
// backward compatibility.
func TestSaveEntityRelationProvenance_VersionChain_DifferentSourceRefEnriches(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	v1, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages",
		Source: "cli", SourceRef: "ref-1",
	})
	if err != nil {
		t.Fatalf("save v1: %v", err)
	}

	// Different source_ref but no valid_from — enriches in place (no version chain).
	v2, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages",
		Source: "cli", SourceRef: "ref-2",
	})
	if err != nil {
		t.Fatalf("save v2: %v", err)
	}
	if v2.ID != v1.ID {
		t.Fatal("without valid_from, different source_ref should enrich in place, not create a new row")
	}
	if v2.SourceRef != "ref-2" {
		t.Fatalf("expected SourceRef updated to ref-2, got %s", v2.SourceRef)
	}
	if v2.ValidFrom != nil {
		t.Fatalf("expected valid_from nil, got %v", v2.ValidFrom)
	}
	if v2.ValidUntil != nil {
		t.Fatalf("expected valid_until nil, got %v", v2.ValidUntil)
	}
}

// TestGraphNeighborsAsOf_ExactBoundary_validFromEqualsAsOf verifies that
// a relation whose valid_from equals the as_of timestamp is included
// (valid_from <= as_of).
func TestGraphNeighborsAsOf_ExactBoundary_validFromEqualsAsOf(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	ts := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		ValidFrom: &ts,
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	_, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &ts)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge when as_of == valid_from, got %d", len(edges))
	}
}

// TestGraphNeighborsAsOf_ExactBoundary_validUntilEqualsAsOf verifies that
// a relation whose valid_until equals the as_of timestamp is excluded
// (valid_until > as_of, not >=).
func TestGraphNeighborsAsOf_ExactBoundary_validUntilEqualsAsOf(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	ts := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	if err := database.SaveEntityRelation(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with",
		ValidUntil: &ts,
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	_, edges, err := database.GraphNeighborsAsOf(ids["Alice"], 1, &ts)
	if err != nil {
		t.Fatalf("GraphNeighborsAsOf: %v", err)
	}
	if len(edges) != 0 {
		t.Fatalf("expected 0 edges when as_of == valid_until (exclusive), got %d", len(edges))
	}
}
