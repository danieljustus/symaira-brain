package db

import (
	"testing"
	"time"
)

// Tests reuse the newTestDB helper from profiles_test.go.

func TestSaveEntity(t *testing.T) {
	db := newTestDB(t)

	tests := []struct {
		name    string
		entity  *Entity
		wantErr bool
	}{
		{
			name: "insert new entity with aliases",
			entity: &Entity{
				ID:          "ent-1",
				Name:        "Alice",
				Type:        "person",
				Aliases:     []string{"Ali", "Al"},
				Description: "A test person",
				CreatedBy:   "tester",
			},
			wantErr: false,
		},
		{
			name: "insert entity with empty aliases",
			entity: &Entity{
				ID:          "ent-2",
				Name:        "ProjectX",
				Type:        "project",
				Aliases:     []string{},
				Description: "A project",
			},
			wantErr: false,
		},
		{
			name: "insert entity with nil aliases",
			entity: &Entity{
				ID:   "ent-3",
				Name: "OrgY",
				Type: "org",
			},
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := db.SaveEntity(tt.entity)
			if (err != nil) != tt.wantErr {
				t.Fatalf("SaveEntity() error = %v, wantErr %v", err, tt.wantErr)
			}
			if err != nil {
				return
			}
			// Verify CreatedAt was set
			if tt.entity.CreatedAt.IsZero() {
				t.Error("expected CreatedAt to be set")
			}
			// Verify UpdatedAt was set
			if tt.entity.UpdatedAt.IsZero() {
				t.Error("expected UpdatedAt to be set")
			}
		})
	}

	// Test upsert: update existing entity
	t.Run("upsert updates existing entity", func(t *testing.T) {
		original := &Entity{
			ID:          "ent-upsert",
			Name:        "Original",
			Type:        "person",
			Aliases:     []string{"Orig"},
			Description: "original desc",
		}
		if err := db.SaveEntity(original); err != nil {
			t.Fatalf("initial save failed: %v", err)
		}
		originalCreatedAt := original.CreatedAt

		// Wait briefly so UpdatedAt differs
		time.Sleep(2 * time.Millisecond)

		updated := &Entity{
			ID:          "ent-upsert",
			Name:        "Updated",
			Type:        "org",
			Aliases:     []string{"Upd", "New"},
			Description: "updated desc",
			CreatedAt:   originalCreatedAt,
		}
		if err := db.SaveEntity(updated); err != nil {
			t.Fatalf("upsert failed: %v", err)
		}

		got, err := db.GetEntityByName("Updated")
		if err != nil {
			t.Fatalf("GetEntityByName failed: %v", err)
		}
		if got == nil {
			t.Fatal("expected entity after upsert, got nil")
		}
		if got.Name != "Updated" {
			t.Errorf("expected name 'Updated', got %q", got.Name)
		}
		if got.Type != "org" {
			t.Errorf("expected type 'org', got %q", got.Type)
		}
		if got.Description != "updated desc" {
			t.Errorf("expected description 'updated desc', got %q", got.Description)
		}
		if len(got.Aliases) != 2 {
			t.Errorf("expected 2 aliases, got %d", len(got.Aliases))
		}
		// Verify old alias was removed and new ones added
		if !got.CreatedAt.Equal(originalCreatedAt) {
			t.Errorf("CreatedAt should be preserved, got %v vs %v", got.CreatedAt, originalCreatedAt)
		}
	})

	// Test that aliases are properly replaced on upsert
	t.Run("upsert replaces aliases", func(t *testing.T) {
		e := &Entity{
			ID:      "ent-alias-replace",
			Name:    "AliasTest",
			Type:    "person",
			Aliases: []string{"Old1", "Old2"},
		}
		if err := db.SaveEntity(e); err != nil {
			t.Fatalf("initial save failed: %v", err)
		}

		// Resolve by old alias should work
		got, err := db.ResolveEntity("Old1")
		if err != nil {
			t.Fatalf("ResolveEntity failed: %v", err)
		}
		if got == nil {
			t.Fatal("expected to resolve by old alias before upsert")
		}

		// Upsert with new aliases
		e.Aliases = []string{"New1"}
		if err := db.SaveEntity(e); err != nil {
			t.Fatalf("upsert failed: %v", err)
		}

		// Old alias should no longer resolve
		got, err = db.ResolveEntity("Old1")
		if err != nil {
			t.Fatalf("ResolveEntity failed: %v", err)
		}
		if got != nil {
			t.Error("expected old alias to not resolve after upsert")
		}

		// New alias should resolve
		got, err = db.ResolveEntity("New1")
		if err != nil {
			t.Fatalf("ResolveEntity failed: %v", err)
		}
		if got == nil {
			t.Error("expected new alias to resolve after upsert")
		}
	})
}

func TestResolveEntity(t *testing.T) {
	db := newTestDB(t)

	// Seed test data
	entities := []*Entity{
		{ID: "re-1", Name: "Alice", Type: "person", Aliases: []string{"Ali", "Al"}},
		{ID: "re-2", Name: "Bob", Type: "person", Aliases: []string{"Bobby"}},
		{ID: "re-3", Name: "ProjectX", Type: "project", Aliases: []string{}},
	}
	for _, e := range entities {
		if err := db.SaveEntity(e); err != nil {
			t.Fatalf("failed to seed entity %s: %v", e.ID, err)
		}
	}

	tests := []struct {
		name    string
		query   string
		wantID  string
		wantNil bool
		wantErr bool
	}{
		{
			name:   "resolve by exact name",
			query:  "Alice",
			wantID: "re-1",
		},
		{
			name:   "resolve by name case-insensitive",
			query:  "alice",
			wantID: "re-1",
		},
		{
			name:   "resolve by name uppercase",
			query:  "ALICE",
			wantID: "re-1",
		},
		{
			name:   "resolve by alias",
			query:  "Ali",
			wantID: "re-1",
		},
		{
			name:   "resolve by alias case-insensitive",
			query:  "ali",
			wantID: "re-1",
		},
		{
			name:   "resolve by second alias",
			query:  "Bobby",
			wantID: "re-2",
		},
		{
			name:   "resolve project by name",
			query:  "ProjectX",
			wantID: "re-3",
		},
		{
			name:    "not found returns nil nil",
			query:   "NonExistent",
			wantNil: true,
		},
		{
			name:    "empty string not found",
			query:   "",
			wantNil: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := db.ResolveEntity(tt.query)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ResolveEntity() error = %v, wantErr %v", err, tt.wantErr)
			}
			if tt.wantNil {
				if got != nil {
					t.Errorf("expected nil, got %+v", got)
				}
				return
			}
			if got == nil {
				t.Fatal("expected entity, got nil")
			}
			if got.ID != tt.wantID {
				t.Errorf("expected ID %q, got %q", tt.wantID, got.ID)
			}
		})
	}
}

func TestGetEntityByName(t *testing.T) {
	db := newTestDB(t)

	// Seed
	if err := db.SaveEntity(&Entity{
		ID:      "gn-1",
		Name:    "Charlie",
		Type:    "person",
		Aliases: []string{"Char", "Chuck"},
	}); err != nil {
		t.Fatalf("seed failed: %v", err)
	}

	tests := []struct {
		name    string
		query   string
		wantID  string
		wantNil bool
		wantErr bool
	}{
		{
			name:   "exact match",
			query:  "Charlie",
			wantID: "gn-1",
		},
		{
			name:   "case insensitive lowercase",
			query:  "charlie",
			wantID: "gn-1",
		},
		{
			name:   "case insensitive uppercase",
			query:  "CHARLIE",
			wantID: "gn-1",
		},
		{
			name:    "not found returns nil",
			query:   "NoSuchEntity",
			wantNil: true,
		},
		{
			name:    "alias does not match GetEntityByName",
			query:   "Char",
			wantNil: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := db.GetEntityByName(tt.query)
			if (err != nil) != tt.wantErr {
				t.Fatalf("GetEntityByName() error = %v, wantErr %v", err, tt.wantErr)
			}
			if tt.wantNil {
				if got != nil {
					t.Errorf("expected nil, got %+v", got)
				}
				return
			}
			if got == nil {
				t.Fatal("expected entity, got nil")
			}
			if got.ID != tt.wantID {
				t.Errorf("expected ID %q, got %q", tt.wantID, got.ID)
			}
			// Verify aliases are properly deserialized
			if len(got.Aliases) != 2 {
				t.Errorf("expected 2 aliases, got %d: %v", len(got.Aliases), got.Aliases)
			}
		})
	}
}

func TestListEntities(t *testing.T) {
	db := newTestDB(t)

	// Empty list
	empty, err := db.ListEntities()
	if err != nil {
		t.Fatalf("ListEntities on empty DB failed: %v", err)
	}
	if len(empty) != 0 {
		t.Errorf("expected 0 entities, got %d", len(empty))
	}

	// Seed entities
	entities := []*Entity{
		{ID: "le-1", Name: "Zara", Type: "person", Aliases: []string{"Z"}},
		{ID: "le-2", Name: "alice", Type: "person", Aliases: []string{}},
		{ID: "le-3", Name: "Bob", Type: "person", Aliases: []string{"Bobby"}},
	}
	for _, e := range entities {
		if err := db.SaveEntity(e); err != nil {
			t.Fatalf("failed to seed entity %s: %v", e.ID, err)
		}
	}

	list, err := db.ListEntities()
	if err != nil {
		t.Fatalf("ListEntities failed: %v", err)
	}
	if len(list) != 3 {
		t.Fatalf("expected 3 entities, got %d", len(list))
	}

	// Verify ordering is case-insensitive by name
	expectedOrder := []string{"alice", "Bob", "Zara"}
	for i, e := range list {
		if e.Name != expectedOrder[i] {
			t.Errorf("list[%d].Name = %q, want %q", i, e.Name, expectedOrder[i])
		}
	}

	// Verify aliases are deserialized
	for _, e := range list {
		if e.Name == "Zara" && len(e.Aliases) != 1 {
			t.Errorf("expected Zara to have 1 alias, got %d", len(e.Aliases))
		}
		if e.Name == "alice" && len(e.Aliases) != 0 {
			t.Errorf("expected alice to have 0 aliases, got %d", len(e.Aliases))
		}
		if e.Name == "Bob" && len(e.Aliases) != 1 {
			t.Errorf("expected Bob to have 1 alias, got %d", len(e.Aliases))
		}
	}
}

func TestDeleteEntity(t *testing.T) {
	db := newTestDB(t)

	// Delete non-existent entity should not error
	if err := db.DeleteEntity("nonexistent"); err != nil {
		t.Fatalf("DeleteEntity on non-existent ID should not error: %v", err)
	}

	// Seed entity
	e := &Entity{
		ID:      "del-1",
		Name:    "ToDelete",
		Type:    "person",
		Aliases: []string{"Del"},
	}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("seed failed: %v", err)
	}

	// Verify it exists
	got, err := db.GetEntityByName("ToDelete")
	if err != nil {
		t.Fatalf("GetEntityByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected entity to exist before delete")
	}

	// Create a memory and link it
	m := &Memory{
		ID:      "del-mem-1",
		Content: "memory linked to entity being deleted",
		Scope:   "global",
	}
	if err := db.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}
	if err := db.LinkMemoryToEntity("del-mem-1", "del-1"); err != nil {
		t.Fatalf("LinkMemoryToEntity failed: %v", err)
	}

	// Verify link exists
	ents, err := db.EntitiesForMemory("del-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory failed: %v", err)
	}
	if len(ents) != 1 {
		t.Fatalf("expected 1 linked entity, got %d", len(ents))
	}

	// Delete entity
	if err := db.DeleteEntity("del-1"); err != nil {
		t.Fatalf("DeleteEntity failed: %v", err)
	}

	// Verify entity is gone
	got, err = db.GetEntityByName("ToDelete")
	if err != nil {
		t.Fatalf("GetEntityByName after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected entity to be nil after deletion")
	}

	// Verify alias no longer resolves
	got, err = db.ResolveEntity("Del")
	if err != nil {
		t.Fatalf("ResolveEntity after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected alias to not resolve after entity deletion")
	}

	// Verify memory link was cleaned up
	ents, err = db.EntitiesForMemory("del-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory after delete failed: %v", err)
	}
	if len(ents) != 0 {
		t.Errorf("expected 0 linked entities after delete, got %d", len(ents))
	}

	// Verify MemoryIDsForEntity returns empty
	ids, err := db.MemoryIDsForEntity("del-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity after delete failed: %v", err)
	}
	if len(ids) != 0 {
		t.Errorf("expected 0 memory IDs after delete, got %d", len(ids))
	}
}

func TestLinkMemoryToEntity(t *testing.T) {
	db := newTestDB(t)

	// Seed entity
	e := &Entity{
		ID:   "link-ent-1",
		Name: "LinkTarget",
		Type: "person",
	}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("seed entity failed: %v", err)
	}

	// Seed memory
	m := &Memory{
		ID:      "link-mem-1",
		Content: "test memory for linking",
		Scope:   "global",
	}
	if err := db.SaveMemory(m); err != nil {
		t.Fatalf("seed memory failed: %v", err)
	}

	// Link
	if err := db.LinkMemoryToEntity("link-mem-1", "link-ent-1"); err != nil {
		t.Fatalf("LinkMemoryToEntity failed: %v", err)
	}

	// Verify link
	ids, err := db.MemoryIDsForEntity("link-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 1 || ids[0] != "link-mem-1" {
		t.Errorf("expected [link-mem-1], got %v", ids)
	}

	// Duplicate link should be ignored (INSERT OR IGNORE)
	if err := db.LinkMemoryToEntity("link-mem-1", "link-ent-1"); err != nil {
		t.Fatalf("duplicate LinkMemoryToEntity should not error: %v", err)
	}
	ids, err = db.MemoryIDsForEntity("link-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity after duplicate failed: %v", err)
	}
	if len(ids) != 1 {
		t.Errorf("expected 1 memory ID after duplicate link, got %d", len(ids))
	}

	// Link multiple memories to same entity
	m2 := &Memory{
		ID:      "link-mem-2",
		Content: "second memory",
		Scope:   "global",
	}
	if err := db.SaveMemory(m2); err != nil {
		t.Fatalf("seed memory 2 failed: %v", err)
	}
	if err := db.LinkMemoryToEntity("link-mem-2", "link-ent-1"); err != nil {
		t.Fatalf("LinkMemoryToEntity for second memory failed: %v", err)
	}

	ids, err = db.MemoryIDsForEntity("link-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 2 {
		t.Errorf("expected 2 memory IDs, got %d", len(ids))
	}
}

func TestUnlinkMemoryFromEntity(t *testing.T) {
	db := newTestDB(t)

	// Seed
	e := &Entity{ID: "unlink-ent-1", Name: "UnlinkTarget", Type: "person"}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("seed entity failed: %v", err)
	}
	m := &Memory{ID: "unlink-mem-1", Content: "test", Scope: "global"}
	if err := db.SaveMemory(m); err != nil {
		t.Fatalf("seed memory failed: %v", err)
	}
	if err := db.LinkMemoryToEntity("unlink-mem-1", "unlink-ent-1"); err != nil {
		t.Fatalf("link failed: %v", err)
	}

	// Verify link exists
	ids, err := db.MemoryIDsForEntity("unlink-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 1 {
		t.Fatalf("expected 1 link before unlink, got %d", len(ids))
	}

	// Unlink
	if err := db.UnlinkMemoryFromEntity("unlink-mem-1", "unlink-ent-1"); err != nil {
		t.Fatalf("UnlinkMemoryFromEntity failed: %v", err)
	}

	// Verify link removed
	ids, err = db.MemoryIDsForEntity("unlink-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity after unlink failed: %v", err)
	}
	if len(ids) != 0 {
		t.Errorf("expected 0 links after unlink, got %d", len(ids))
	}

	// Unlink non-existent should not error
	if err := db.UnlinkMemoryFromEntity("nonexistent-mem", "nonexistent-ent"); err != nil {
		t.Fatalf("UnlinkMemoryFromEntity on non-existent should not error: %v", err)
	}
}

func TestEntitiesForMemory(t *testing.T) {
	db := newTestDB(t)

	// No links yet
	ents, err := db.EntitiesForMemory("no-such-memory")
	if err != nil {
		t.Fatalf("EntitiesForMemory failed: %v", err)
	}
	if len(ents) != 0 {
		t.Errorf("expected 0 entities for unknown memory, got %d", len(ents))
	}

	// Seed entities
	entities := []*Entity{
		{ID: "efm-1", Name: "Zara", Type: "person", Aliases: []string{"Z"}},
		{ID: "efm-2", Name: "alice", Type: "person", Aliases: []string{}},
		{ID: "efm-3", Name: "Bob", Type: "project", Aliases: []string{"Bobby"}},
	}
	for _, e := range entities {
		if err := db.SaveEntity(e); err != nil {
			t.Fatalf("seed entity failed: %v", err)
		}
	}

	// Seed memory
	m := &Memory{ID: "efm-mem-1", Content: "test", Scope: "global"}
	if err := db.SaveMemory(m); err != nil {
		t.Fatalf("seed memory failed: %v", err)
	}

	// Link all three
	for _, e := range entities {
		if err := db.LinkMemoryToEntity("efm-mem-1", e.ID); err != nil {
			t.Fatalf("link failed: %v", err)
		}
	}

	// Get entities for memory
	ents, err = db.EntitiesForMemory("efm-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory failed: %v", err)
	}
	if len(ents) != 3 {
		t.Fatalf("expected 3 entities, got %d", len(ents))
	}

	// Verify ordering (case-insensitive by name)
	expectedOrder := []string{"alice", "Bob", "Zara"}
	for i, e := range ents {
		if e.Name != expectedOrder[i] {
			t.Errorf("ents[%d].Name = %q, want %q", i, e.Name, expectedOrder[i])
		}
	}

	// Verify aliases are deserialized
	for _, e := range ents {
		if e.Name == "Zara" {
			if len(e.Aliases) != 1 || e.Aliases[0] != "Z" {
				t.Errorf("expected Zara aliases [Z], got %v", e.Aliases)
			}
		}
		if e.Name == "Bob" {
			if len(e.Aliases) != 1 || e.Aliases[0] != "Bobby" {
				t.Errorf("expected Bob aliases [Bobby], got %v", e.Aliases)
			}
		}
	}

	// Unlink one and verify
	if err := db.UnlinkMemoryFromEntity("efm-mem-1", "efm-2"); err != nil {
		t.Fatalf("unlink failed: %v", err)
	}
	ents, err = db.EntitiesForMemory("efm-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory after unlink failed: %v", err)
	}
	if len(ents) != 2 {
		t.Errorf("expected 2 entities after unlink, got %d", len(ents))
	}
}

func TestMemoryIDsForEntity(t *testing.T) {
	db := newTestDB(t)

	// No links yet
	ids, err := db.MemoryIDsForEntity("no-such-entity")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 0 {
		t.Errorf("expected 0 IDs for unknown entity, got %d", len(ids))
	}

	// Seed
	e := &Entity{ID: "mie-1", Name: "TestEntity", Type: "person"}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("seed entity failed: %v", err)
	}

	// Empty initially
	ids, err = db.MemoryIDsForEntity("mie-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 0 {
		t.Errorf("expected 0 IDs initially, got %d", len(ids))
	}

	// Link multiple memories
	memIDs := []string{"mie-mem-1", "mie-mem-2", "mie-mem-3"}
	for _, mid := range memIDs {
		m := &Memory{ID: mid, Content: "content " + mid, Scope: "global"}
		if err := db.SaveMemory(m); err != nil {
			t.Fatalf("seed memory %s failed: %v", mid, err)
		}
		if err := db.LinkMemoryToEntity(mid, "mie-1"); err != nil {
			t.Fatalf("link %s failed: %v", mid, err)
		}
	}

	ids, err = db.MemoryIDsForEntity("mie-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity failed: %v", err)
	}
	if len(ids) != 3 {
		t.Fatalf("expected 3 IDs, got %d", len(ids))
	}

	// Verify all IDs are present
	idSet := make(map[string]bool)
	for _, id := range ids {
		idSet[id] = true
	}
	for _, mid := range memIDs {
		if !idSet[mid] {
			t.Errorf("expected memory ID %q in results", mid)
		}
	}

	// Unlink one and verify
	if err := db.UnlinkMemoryFromEntity("mie-mem-2", "mie-1"); err != nil {
		t.Fatalf("unlink failed: %v", err)
	}
	ids, err = db.MemoryIDsForEntity("mie-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity after unlink failed: %v", err)
	}
	if len(ids) != 2 {
		t.Errorf("expected 2 IDs after unlink, got %d", len(ids))
	}
}

func TestEntityCRUDRoundTrip(t *testing.T) {
	db := newTestDB(t)

	// Create
	e := &Entity{
		ID:          "crud-1",
		Name:        "TestPerson",
		Type:        "person",
		Aliases:     []string{"TP", "Tester"},
		Description: "A test person for CRUD",
		CreatedBy:   "test-suite",
	}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("SaveEntity failed: %v", err)
	}
	if e.CreatedAt.IsZero() || e.UpdatedAt.IsZero() {
		t.Error("expected timestamps to be set after create")
	}

	// Read by name
	got, err := db.GetEntityByName("TestPerson")
	if err != nil {
		t.Fatalf("GetEntityByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected entity, got nil")
	}
	if got.ID != "crud-1" {
		t.Errorf("expected ID 'crud-1', got %q", got.ID)
	}
	if got.Type != "person" {
		t.Errorf("expected type 'person', got %q", got.Type)
	}
	if got.Description != "A test person for CRUD" {
		t.Errorf("expected description, got %q", got.Description)
	}
	if got.CreatedBy != "test-suite" {
		t.Errorf("expected created_by 'test-suite', got %q", got.CreatedBy)
	}
	if len(got.Aliases) != 2 {
		t.Errorf("expected 2 aliases, got %d", len(got.Aliases))
	}

	// Read by alias
	got, err = db.ResolveEntity("TP")
	if err != nil {
		t.Fatalf("ResolveEntity by alias failed: %v", err)
	}
	if got == nil || got.ID != "crud-1" {
		t.Errorf("expected to resolve entity by alias 'TP'")
	}

	// Update
	time.Sleep(2 * time.Millisecond)
	e.Description = "Updated description"
	e.Aliases = []string{"TP", "Tester", "NewAlias"}
	if err := db.SaveEntity(e); err != nil {
		t.Fatalf("SaveEntity update failed: %v", err)
	}

	got, err = db.GetEntityByName("TestPerson")
	if err != nil {
		t.Fatalf("GetEntityByName after update failed: %v", err)
	}
	if got.Description != "Updated description" {
		t.Errorf("expected updated description, got %q", got.Description)
	}
	if len(got.Aliases) != 3 {
		t.Errorf("expected 3 aliases after update, got %d", len(got.Aliases))
	}

	// List
	list, err := db.ListEntities()
	if err != nil {
		t.Fatalf("ListEntities failed: %v", err)
	}
	found := false
	for _, ent := range list {
		if ent.ID == "crud-1" {
			found = true
			break
		}
	}
	if !found {
		t.Error("expected entity in list")
	}

	// Delete
	if err := db.DeleteEntity("crud-1"); err != nil {
		t.Fatalf("DeleteEntity failed: %v", err)
	}
	got, err = db.GetEntityByName("TestPerson")
	if err != nil {
		t.Fatalf("GetEntityByName after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected nil after delete")
	}
}

func TestEntityMemoryLinkingRoundTrip(t *testing.T) {
	db := newTestDB(t)

	// Create two entities
	e1 := &Entity{ID: "rt-ent-1", Name: "Entity1", Type: "person"}
	e2 := &Entity{ID: "rt-ent-2", Name: "Entity2", Type: "project"}
	if err := db.SaveEntity(e1); err != nil {
		t.Fatalf("save e1 failed: %v", err)
	}
	if err := db.SaveEntity(e2); err != nil {
		t.Fatalf("save e2 failed: %v", err)
	}

	// Create two memories
	m1 := &Memory{ID: "rt-mem-1", Content: "memory one", Scope: "global"}
	m2 := &Memory{ID: "rt-mem-2", Content: "memory two", Scope: "global"}
	if err := db.SaveMemory(m1); err != nil {
		t.Fatalf("save m1 failed: %v", err)
	}
	if err := db.SaveMemory(m2); err != nil {
		t.Fatalf("save m2 failed: %v", err)
	}

	// Link: m1 -> e1, m1 -> e2, m2 -> e1
	if err := db.LinkMemoryToEntity("rt-mem-1", "rt-ent-1"); err != nil {
		t.Fatalf("link m1-e1 failed: %v", err)
	}
	if err := db.LinkMemoryToEntity("rt-mem-1", "rt-ent-2"); err != nil {
		t.Fatalf("link m1-e2 failed: %v", err)
	}
	if err := db.LinkMemoryToEntity("rt-mem-2", "rt-ent-1"); err != nil {
		t.Fatalf("link m2-e1 failed: %v", err)
	}

	// EntitiesForMemory(m1) should return e1, e2
	ents, err := db.EntitiesForMemory("rt-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory(m1) failed: %v", err)
	}
	if len(ents) != 2 {
		t.Fatalf("expected 2 entities for m1, got %d", len(ents))
	}

	// EntitiesForMemory(m2) should return e1
	ents, err = db.EntitiesForMemory("rt-mem-2")
	if err != nil {
		t.Fatalf("EntitiesForMemory(m2) failed: %v", err)
	}
	if len(ents) != 1 || ents[0].ID != "rt-ent-1" {
		t.Errorf("expected [rt-ent-1] for m2, got %v", ents)
	}

	// MemoryIDsForEntity(e1) should return m1, m2
	ids, err := db.MemoryIDsForEntity("rt-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity(e1) failed: %v", err)
	}
	if len(ids) != 2 {
		t.Errorf("expected 2 memory IDs for e1, got %d", len(ids))
	}

	// MemoryIDsForEntity(e2) should return m1
	ids, err = db.MemoryIDsForEntity("rt-ent-2")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity(e2) failed: %v", err)
	}
	if len(ids) != 1 || ids[0] != "rt-mem-1" {
		t.Errorf("expected [rt-mem-1] for e2, got %v", ids)
	}

	// Unlink m1-e1
	if err := db.UnlinkMemoryFromEntity("rt-mem-1", "rt-ent-1"); err != nil {
		t.Fatalf("unlink failed: %v", err)
	}

	// Verify: m1 now only has e2
	ents, err = db.EntitiesForMemory("rt-mem-1")
	if err != nil {
		t.Fatalf("EntitiesForMemory after unlink failed: %v", err)
	}
	if len(ents) != 1 || ents[0].ID != "rt-ent-2" {
		t.Errorf("expected [rt-ent-2] for m1 after unlink, got %v", ents)
	}

	// Verify: e1 now only has m2
	ids, err = db.MemoryIDsForEntity("rt-ent-1")
	if err != nil {
		t.Fatalf("MemoryIDsForEntity after unlink failed: %v", err)
	}
	if len(ids) != 1 || ids[0] != "rt-mem-2" {
		t.Errorf("expected [rt-mem-2] for e1 after unlink, got %v", ids)
	}
}
