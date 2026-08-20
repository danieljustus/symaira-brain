package db

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// entityAuditEvents returns audit events for the given action and target id.
func entityAuditEvents(t *testing.T, database *DB, action, targetType, targetID string) []*AuditEvent {
	t.Helper()
	events, err := database.GetAuditLogs(action, 100)
	if err != nil {
		t.Fatalf("GetAuditLogs failed: %v", err)
	}
	var out []*AuditEvent
	for _, e := range events {
		if e.TargetType == targetType && (targetID == "" || e.TargetID == targetID) {
			out = append(out, e)
		}
	}
	return out
}

func TestEntityCreateAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	e := &Entity{ID: "ent-1", Name: "auth-service", Type: "service", CreatedBy: "cli:test"}
	if err := database.SaveEntity(e); err != nil {
		t.Fatalf("SaveEntity failed: %v", err)
	}

	events := entityAuditEvents(t, database, "entity_create", TargetEntity, "ent-1")
	if len(events) != 1 {
		t.Fatalf("expected exactly 1 entity_create event, got %d", len(events))
	}
	ev := events[0]
	if ev.TargetType != TargetEntity || ev.TargetID != "ent-1" {
		t.Errorf("unexpected target: type=%q id=%q", ev.TargetType, ev.TargetID)
	}
	if ev.MemoryID != "" {
		t.Errorf("entity event must not reuse memory_id, got %q", ev.MemoryID)
	}
	if ev.Actor != "cli:test" {
		t.Errorf("expected actor cli:test, got %q", ev.Actor)
	}
	if !strings.Contains(ev.Detail, "auth-service") {
		t.Errorf("detail missing entity name: %s", ev.Detail)
	}
}

func TestEntityRenameAndAliasAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	e := &Entity{ID: "ent-2", Name: "old-name", Type: "service", Aliases: []string{"on"}, CreatedBy: "cli:test"}
	if err := database.SaveEntity(e); err != nil {
		t.Fatalf("initial SaveEntity failed: %v", err)
	}

	e.Name = "new-name"
	e.Aliases = []string{"on", "added"}
	e.CreatedBy = "cli:second"
	if err := database.SaveEntity(e); err != nil {
		t.Fatalf("update SaveEntity failed: %v", err)
	}

	events := entityAuditEvents(t, database, "entity_update", TargetEntity, "ent-2")
	if len(events) != 1 {
		t.Fatalf("expected exactly 1 entity_update event, got %d", len(events))
	}
	ev := events[0]
	var detail map[string]interface{}
	if err := json.Unmarshal([]byte(ev.Detail), &detail); err != nil {
		t.Fatalf("detail is not JSON: %v", err)
	}
	if detail["renamed_from"] != "old-name" {
		t.Errorf("expected renamed_from old-name, got %v", detail["renamed_from"])
	}
	added, _ := detail["aliases_added"].([]interface{})
	if len(added) != 1 || added[0] != "added" {
		t.Errorf("expected aliases_added [added], got %v", detail["aliases_added"])
	}
	if ev.Actor != "cli:second" {
		t.Errorf("expected actor from the writing caller, got %q", ev.Actor)
	}
}

func TestEntityDeleteAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	e := &Entity{ID: "ent-3", Name: "doomed", Type: "tool", CreatedBy: "cli:test"}
	if err := database.SaveEntity(e); err != nil {
		t.Fatalf("SaveEntity failed: %v", err)
	}
	if err := database.DeleteEntity("ent-3"); err != nil {
		t.Fatalf("DeleteEntity failed: %v", err)
	}

	events := entityAuditEvents(t, database, "entity_delete", TargetEntity, "ent-3")
	if len(events) != 1 {
		t.Fatalf("expected exactly 1 entity_delete event, got %d", len(events))
	}
	if !strings.Contains(events[0].Detail, "doomed") {
		t.Errorf("delete detail missing name: %s", events[0].Detail)
	}
}

func TestEntityDeleteMissingNoAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	if err := database.DeleteEntity("does-not-exist"); err != nil {
		t.Fatalf("DeleteEntity failed: %v", err)
	}
	events, err := database.GetAuditLogs("entity_delete", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 0 {
		t.Errorf("deleting a missing entity must not emit an audit event, got %d", len(events))
	}
}

func TestRelationCreateAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	for _, ent := range []*Entity{
		{ID: "rel-a", Name: "alpha", Type: "service", CreatedBy: "cli:test"},
		{ID: "rel-b", Name: "beta", Type: "service", CreatedBy: "cli:test"},
	} {
		if err := database.SaveEntity(ent); err != nil {
			t.Fatalf("SaveEntity failed: %v", err)
		}
	}

	r := &EntityRelation{
		FromEntityID: "rel-a",
		ToEntityID:   "rel-b",
		RelationType: "depends_on",
		Source:       "scanner",
		SourceRef:    "ref-1",
		Verification: VerificationVerified,
		Evidence:     `{"found":true}`,
		CreatedBy:    "mcp:agent-x",
	}
	saved, err := database.SaveEntityRelationProvenance(r)
	if err != nil {
		t.Fatalf("SaveEntityRelationProvenance failed: %v", err)
	}

	events := entityAuditEvents(t, database, "relation_create", TargetRelation, saved.ID)
	if len(events) != 1 {
		t.Fatalf("expected exactly 1 relation_create event, got %d", len(events))
	}
	ev := events[0]
	if ev.TargetType != TargetRelation || ev.TargetID != saved.ID {
		t.Errorf("unexpected target: type=%q id=%q", ev.TargetType, ev.TargetID)
	}
	if ev.Actor != "mcp:agent-x" {
		t.Errorf("expected actor mcp:agent-x, got %q", ev.Actor)
	}
	for _, want := range []string{"rel-a", "rel-b", "depends_on", "scanner", "ref-1"} {
		if !strings.Contains(ev.Detail, want) {
			t.Errorf("relation detail missing %q: %s", want, ev.Detail)
		}
	}
}

// saveRelationEntities creates the two entities a relation test needs, so
// the entity_relations foreign keys resolve.
func saveRelationEntities(t *testing.T, database *DB) {
	t.Helper()
	for _, ent := range []*Entity{
		{ID: "rel-a", Name: "alpha", Type: "service", CreatedBy: "cli:test"},
		{ID: "rel-b", Name: "beta", Type: "service", CreatedBy: "cli:test"},
	} {
		if err := database.SaveEntity(ent); err != nil {
			t.Fatalf("SaveEntity failed: %v", err)
		}
	}
}

func TestRelationIdempotentRetryNoDuplicateAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()
	saveRelationEntities(t, database)

	r := &EntityRelation{
		FromEntityID: "rel-a",
		ToEntityID:   "rel-b",
		RelationType: "depends_on",
		Source:       "scanner",
		SourceRef:    "ref-1",
		CreatedBy:    "mcp:agent-x",
	}
	first, err := database.SaveEntityRelationProvenance(r)
	if err != nil {
		t.Fatalf("first save failed: %v", err)
	}
	second, err := database.SaveEntityRelationProvenance(r)
	if err != nil {
		t.Fatalf("idempotent retry failed: %v", err)
	}
	if first.ID != second.ID {
		t.Fatalf("idempotent retry returned a different relation: %s vs %s", first.ID, second.ID)
	}

	events := entityAuditEvents(t, database, "relation_create", TargetRelation, first.ID)
	if len(events) != 1 {
		t.Fatalf("idempotent retry must not emit a duplicate event, got %d", len(events))
	}
}

func TestRelationUpdateAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()
	saveRelationEntities(t, database)

	r := &EntityRelation{
		FromEntityID: "rel-a",
		ToEntityID:   "rel-b",
		RelationType: "depends_on",
		CreatedBy:    "mcp:agent-x",
	}
	if err := database.SaveEntityRelation(r); err != nil {
		t.Fatalf("SaveEntityRelation failed: %v", err)
	}
	savedID := r.ID
	if savedID == "" {
		t.Fatal("SaveEntityRelation did not assign an ID")
	}

	// Enrich the unverified relation with provenance — an update, not a create.
	enriched := &EntityRelation{
		FromEntityID: "rel-a",
		ToEntityID:   "rel-b",
		RelationType: "depends_on",
		Source:       "scanner",
		SourceRef:    "ref-2",
		Verification: VerificationUnverified,
		Evidence:     `{"found":true}`,
		CreatedBy:    "mcp:agent-y",
	}
	if _, err := database.SaveEntityRelationProvenance(enriched); err != nil {
		t.Fatalf("enrich save failed: %v", err)
	}

	creates := entityAuditEvents(t, database, "relation_create", TargetRelation, savedID)
	if len(creates) != 1 {
		t.Fatalf("expected exactly 1 relation_create, got %d", len(creates))
	}
	updates := entityAuditEvents(t, database, "relation_update", TargetRelation, savedID)
	if len(updates) != 1 {
		t.Fatalf("expected exactly 1 relation_update, got %d", len(updates))
	}
	if !strings.Contains(updates[0].Detail, "ref-2") {
		t.Errorf("update detail missing new provenance: %s", updates[0].Detail)
	}
}

func TestRelationDeleteAudit(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()
	saveRelationEntities(t, database)

	r := &EntityRelation{
		FromEntityID: "rel-a",
		ToEntityID:   "rel-b",
		RelationType: "depends_on",
		CreatedBy:    "mcp:agent-x",
	}
	if err := database.SaveEntityRelation(r); err != nil {
		t.Fatalf("SaveEntityRelation failed: %v", err)
	}
	savedID := r.ID

	if err := database.DeleteEntityRelation("rel-a", "rel-b", "depends_on"); err != nil {
		t.Fatalf("DeleteEntityRelation failed: %v", err)
	}
	events := entityAuditEvents(t, database, "relation_delete", TargetRelation, savedID)
	if len(events) != 1 {
		t.Fatalf("expected exactly 1 relation_delete, got %d", len(events))
	}
	if !strings.Contains(events[0].Detail, "depends_on") {
		t.Errorf("delete detail missing triple: %s", events[0].Detail)
	}

	// Deleting a missing relation emits nothing.
	if err := database.DeleteEntityRelation("rel-a", "rel-b", "depends_on"); err != nil {
		t.Fatalf("second DeleteEntityRelation failed: %v", err)
	}
	if events = entityAuditEvents(t, database, "relation_delete", TargetRelation, savedID); len(events) != 1 {
		t.Errorf("deleting a missing relation must not emit a new event, got %d", len(events))
	}

	// DeleteEntityRelationByID path.
	r2 := &EntityRelation{FromEntityID: "rel-a", ToEntityID: "rel-b", RelationType: "related_to", CreatedBy: "mcp:agent-x"}
	if err := database.SaveEntityRelation(r2); err != nil {
		t.Fatalf("second SaveEntityRelation failed: %v", err)
	}
	saved2ID := r2.ID
	if err := database.DeleteEntityRelationByID(saved2ID); err != nil {
		t.Fatalf("DeleteEntityRelationByID failed: %v", err)
	}
	if events = entityAuditEvents(t, database, "relation_delete", TargetRelation, saved2ID); len(events) != 1 {
		t.Errorf("expected 1 relation_delete for ByID path, got %d", len(events))
	}
}

func TestEntityAuditDisabled(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	database.SetAuditLogEnabled(false)
	defer database.SetAuditLogEnabled(true)

	e := &Entity{ID: "ent-off", Name: "quiet", Type: "tool", CreatedBy: "cli:test"}
	if err := database.SaveEntity(e); err != nil {
		t.Fatalf("SaveEntity with audit disabled failed: %v", err)
	}
	r := &EntityRelation{FromEntityID: "ent-off", ToEntityID: "ent-off", RelationType: "self", CreatedBy: "mcp:x"}
	if err := database.SaveEntityRelation(r); err != nil {
		t.Fatalf("SaveEntityRelation with audit disabled failed: %v", err)
	}
	if err := database.DeleteEntity("ent-off"); err != nil {
		t.Fatalf("DeleteEntity with audit disabled failed: %v", err)
	}

	for _, action := range []string{"entity_create", "entity_update", "entity_delete", "relation_create", "relation_update", "relation_delete"} {
		events, err := database.GetAuditLogs(action, 10)
		if err != nil {
			t.Fatal(err)
		}
		if len(events) != 0 {
			t.Errorf("audit disabled: %s events were written (%d)", action, len(events))
		}
	}
}

func TestAuditLogTargetRoundTrip(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	// Memory event keeps memory_id and no target.
	if err := database.LogAudit("set", "mem-1", "global", "s1", "u1", ""); err != nil {
		t.Fatal(err)
	}
	// Entity event keeps target and no memory_id.
	if err := database.LogTargetAudit("entity_create", TargetEntity, "ent-x", "", "", "u2", `{"name":"x"}`); err != nil {
		t.Fatal(err)
	}

	events, err := database.GetAuditLogs("", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}

	memEv := events[1]
	if memEv.MemoryID != "mem-1" || memEv.TargetType != "" || memEv.TargetID != "" {
		t.Errorf("memory event misread: memory_id=%q target=%q/%q", memEv.MemoryID, memEv.TargetType, memEv.TargetID)
	}
	entEv := events[0]
	if entEv.MemoryID != "" || entEv.TargetType != TargetEntity || entEv.TargetID != "ent-x" {
		t.Errorf("entity event misread: memory_id=%q target=%q/%q", entEv.MemoryID, entEv.TargetType, entEv.TargetID)
	}
}

func TestAuditLogPruningKeepsTargetEvents(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	if err := database.LogTargetAudit("entity_create", TargetEntity, "ent-keep", "", "", "u1", ""); err != nil {
		t.Fatal(err)
	}
	// Purge audit pruning only removes rows older than the retention
	// window; a fresh event must survive a prune call with a long retention.
	if _, err := database.PurgeExpiredAuditLogs(1000 * time.Hour); err != nil {
		t.Fatal(err)
	}
	events := entityAuditEvents(t, database, "entity_create", TargetEntity, "ent-keep")
	if len(events) != 1 {
		t.Errorf("expected 1 surviving entity event, got %d", len(events))
	}
}
