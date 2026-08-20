package db

import (
	"testing"
)

func seedRelationEntities(t *testing.T, database *DB, names ...string) map[string]string {
	t.Helper()
	ids := make(map[string]string)
	for _, name := range names {
		e := &Entity{ID: "ent-" + name, Name: name, Type: "person"}
		if err := database.SaveEntity(e); err != nil {
			t.Fatalf("seed entity %s: %v", name, err)
		}
		ids[name] = e.ID
	}
	return ids
}

func TestSaveEntityRelation_RoundTrip(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with", CreatedBy: "tester"}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("SaveEntityRelation: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 || out[0].ToEntityID != ids["Bob"] || out[0].RelationType != "works-with" {
		t.Fatalf("unexpected outgoing relations: %+v", out)
	}

	in, err := database.IncomingRelations(ids["Bob"])
	if err != nil {
		t.Fatalf("IncomingRelations: %v", err)
	}
	if len(in) != 1 || in[0].FromEntityID != ids["Alice"] {
		t.Fatalf("unexpected incoming relations: %+v", in)
	}

	// Directed: Bob has no outgoing relation to Alice.
	bobOut, err := database.OutgoingRelations(ids["Bob"])
	if err != nil {
		t.Fatalf("OutgoingRelations(Bob): %v", err)
	}
	if len(bobOut) != 0 {
		t.Fatalf("expected 0 outgoing relations for Bob, got %d", len(bobOut))
	}
}

func TestSaveEntityRelation_IdempotentOnDuplicate(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("first save: %v", err)
	}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("second save: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected exactly 1 relation after duplicate save, got %d", len(out))
	}
}

func TestSaveEntityRelation_DistinctRelationTypesCoexist(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}); err != nil {
		t.Fatalf("save works-with: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "manages"}); err != nil {
		t.Fatalf("save manages: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 2 {
		t.Fatalf("expected 2 distinct relation types, got %d", len(out))
	}
}

func TestDeleteEntityRelation(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("save: %v", err)
	}
	if err := database.DeleteEntityRelation(ids["Alice"], ids["Bob"], "works-with"); err != nil {
		t.Fatalf("delete: %v", err)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 0 {
		t.Fatalf("expected 0 relations after delete, got %d", len(out))
	}
}

func TestRelationsForEntity_CombinesBothDirections(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob", "Carol")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}); err != nil {
		t.Fatalf("save Alice->Bob: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Carol"], ToEntityID: ids["Bob"], RelationType: "reports-to"}); err != nil {
		t.Fatalf("save Carol->Bob: %v", err)
	}

	all, err := database.RelationsForEntity(ids["Bob"])
	if err != nil {
		t.Fatalf("RelationsForEntity: %v", err)
	}
	if len(all) != 2 {
		t.Fatalf("expected 2 relations touching Bob, got %d", len(all))
	}
}

func TestGraphNeighbors_Depth1(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob", "Carol")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}); err != nil {
		t.Fatalf("save Alice->Bob: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Bob"], ToEntityID: ids["Carol"], RelationType: "manages"}); err != nil {
		t.Fatalf("save Bob->Carol: %v", err)
	}

	nodes, edges, err := database.GraphNeighbors(ids["Alice"], 1)
	if err != nil {
		t.Fatalf("GraphNeighbors: %v", err)
	}
	if len(nodes) != 2 {
		t.Fatalf("expected 2 nodes at depth 1 (Alice, Bob), got %d", len(nodes))
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge at depth 1, got %d", len(edges))
	}
}

func TestGraphNeighbors_Depth2ReachesSecondHop(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob", "Carol")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}); err != nil {
		t.Fatalf("save Alice->Bob: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Bob"], ToEntityID: ids["Carol"], RelationType: "manages"}); err != nil {
		t.Fatalf("save Bob->Carol: %v", err)
	}

	nodes, edges, err := database.GraphNeighbors(ids["Alice"], 2)
	if err != nil {
		t.Fatalf("GraphNeighbors: %v", err)
	}
	if len(nodes) != 3 {
		t.Fatalf("expected 3 nodes at depth 2 (Alice, Bob, Carol), got %d", len(nodes))
	}
	if len(edges) != 2 {
		t.Fatalf("expected 2 edges at depth 2, got %d", len(edges))
	}
}

func TestGraphNeighbors_CycleTerminatesAndDeduplicates(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "A", "B", "C")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["A"], ToEntityID: ids["B"], RelationType: "next"}); err != nil {
		t.Fatalf("save A->B: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["B"], ToEntityID: ids["C"], RelationType: "next"}); err != nil {
		t.Fatalf("save B->C: %v", err)
	}
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["C"], ToEntityID: ids["A"], RelationType: "next"}); err != nil {
		t.Fatalf("save C->A: %v", err)
	}

	nodes, edges, err := database.GraphNeighbors(ids["A"], MaxGraphDepth)
	if err != nil {
		t.Fatalf("GraphNeighbors: %v", err)
	}
	if len(nodes) != 3 {
		t.Fatalf("expected exactly 3 distinct nodes in the A->B->C->A cycle, got %d", len(nodes))
	}
	if len(edges) != 3 {
		t.Fatalf("expected exactly 3 distinct edges in the cycle, got %d", len(edges))
	}
}

func TestGraphNeighbors_DepthBeyondCapIsRejected(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice")

	_, _, err := database.GraphNeighbors(ids["Alice"], MaxGraphDepth+1)
	if err == nil {
		t.Fatal("expected an error for depth beyond the configured cap")
	}
}

func TestEntityRelation_CascadesOnEntityDelete(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}); err != nil {
		t.Fatalf("save: %v", err)
	}
	if err := database.DeleteEntity(ids["Alice"]); err != nil {
		t.Fatalf("delete entity: %v", err)
	}

	in, err := database.IncomingRelations(ids["Bob"])
	if err != nil {
		t.Fatalf("IncomingRelations: %v", err)
	}
	if len(in) != 0 {
		t.Fatalf("expected relation to be cascade-deleted with its entity, got %d remaining", len(in))
	}
}
