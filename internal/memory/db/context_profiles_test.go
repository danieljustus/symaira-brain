package db

import (
	"strings"
	"testing"

	"github.com/google/uuid"
)

func newTestContextProfile(t *testing.T) *DB {
	t.Helper()
	return newTestDB(t)
}

func strPtr(s string) *string { return &s }

func TestSaveContextProfile_Insert(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{
		ID:          uuid.New().String(),
		Name:        "dev-context",
		Description: "Development context profile",
		BaseScope:   "project",
	}

	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}
	if cp.CreatedAt.IsZero() {
		t.Error("expected CreatedAt to be set")
	}
}

func TestGetContextProfileByName_Found(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{
		ID:        uuid.New().String(),
		Name:      "test-profile",
		BaseScope: "global",
	}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}

	got, err := db.GetContextProfileByName("test-profile")
	if err != nil {
		t.Fatalf("GetContextProfileByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected profile, got nil")
	}
	if got.Name != "test-profile" {
		t.Errorf("expected name 'test-profile', got %q", got.Name)
	}
}

func TestGetContextProfileByName_NotFound(t *testing.T) {
	db := newTestContextProfile(t)

	got, err := db.GetContextProfileByName("nonexistent")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil, got %+v", got)
	}
}

func TestListContextProfiles_Empty(t *testing.T) {
	db := newTestContextProfile(t)

	profiles, err := db.ListContextProfiles()
	if err != nil {
		t.Fatalf("ListContextProfiles failed: %v", err)
	}
	if len(profiles) != 0 {
		t.Errorf("expected 0 profiles, got %d", len(profiles))
	}
}

func TestListContextProfiles_Multiple(t *testing.T) {
	db := newTestContextProfile(t)

	names := []string{"Alpha", "beta", "Gamma"}
	for _, name := range names {
		cp := &ContextProfile{
			ID:   uuid.New().String(),
			Name: name,
		}
		if err := db.SaveContextProfile(cp); err != nil {
			t.Fatalf("SaveContextProfile(%s) failed: %v", name, err)
		}
	}

	profiles, err := db.ListContextProfiles()
	if err != nil {
		t.Fatalf("ListContextProfiles failed: %v", err)
	}
	if len(profiles) != 3 {
		t.Fatalf("expected 3 profiles, got %d", len(profiles))
	}

	expectedOrder := []string{"Alpha", "beta", "Gamma"}
	for i, p := range profiles {
		if p.Name != expectedOrder[i] {
			t.Errorf("profile[%d] name = %q, want %q", i, p.Name, expectedOrder[i])
		}
	}
}

func TestDeleteContextProfile_Cascade(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{
		ID:   uuid.New().String(),
		Name: "to-delete",
	}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}

	link := &ContextProfileLink{
		ProfileID:       cp.ID,
		Scope:           "global",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(link); err != nil {
		t.Fatalf("AddContextProfileLink failed: %v", err)
	}

	if err := db.DeleteContextProfile("to-delete"); err != nil {
		t.Fatalf("DeleteContextProfile failed: %v", err)
	}

	got, _ := db.GetContextProfileByName("to-delete")
	if got != nil {
		t.Error("expected profile to be deleted")
	}
}

func TestContextProfileLinks_CRUD(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{
		ID:   uuid.New().String(),
		Name: "link-test",
	}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}

	l1 := &ContextProfileLink{
		ProfileID:       cp.ID,
		Scope:           "global",
		PrecedenceOrder: 2,
	}
	l2 := &ContextProfileLink{
		ProfileID:       cp.ID,
		Scope:           "project",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(l1); err != nil {
		t.Fatalf("AddContextProfileLink l1 failed: %v", err)
	}
	if err := db.AddContextProfileLink(l2); err != nil {
		t.Fatalf("AddContextProfileLink l2 failed: %v", err)
	}

	links, err := db.ListContextProfileLinks("link-test")
	if err != nil {
		t.Fatalf("ListContextProfileLinks failed: %v", err)
	}
	if len(links) != 2 {
		t.Fatalf("expected 2 links, got %d", len(links))
	}
	if links[0].Scope != "project" || links[1].Scope != "global" {
		t.Errorf("expected [project, global] order, got [%s, %s]", links[0].Scope, links[1].Scope)
	}

	if err := db.RemoveContextProfileLink("link-test", "global"); err != nil {
		t.Fatalf("RemoveContextProfileLink failed: %v", err)
	}

	links, _ = db.ListContextProfileLinks("link-test")
	if len(links) != 1 {
		t.Fatalf("expected 1 link after removal, got %d", len(links))
	}
}

func TestResolveContextProfile_MultipleLinks(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{
		ID:        uuid.New().String(),
		Name:      "multi",
		BaseScope: "session",
	}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}

	for i, scope := range []string{"global", "project", "agent"} {
		link := &ContextProfileLink{
			ProfileID:       cp.ID,
			Scope:           scope,
			PrecedenceOrder: i + 1,
		}
		if err := db.AddContextProfileLink(link); err != nil {
			t.Fatalf("AddContextProfileLink(%s) failed: %v", scope, err)
		}
	}

	scopes, err := db.ResolveContextProfile("multi", DefaultMaxDepth)
	if err != nil {
		t.Fatalf("ResolveContextProfile failed: %v", err)
	}
	if len(scopes) != 4 {
		t.Fatalf("expected 4 resolved scopes (3 links + 1 base), got %d", len(scopes))
	}

	expected := []string{"global", "project", "agent", "session"}
	for i, s := range scopes {
		if s.Scope != expected[i] {
			t.Errorf("resolved scope[%d] = %q, want %q", i, s.Scope, expected[i])
		}
		if s.Profile != "multi" {
			t.Errorf("resolved scope[%d].Profile = %q, want 'multi'", i, s.Profile)
		}
	}
}

func TestResolveContextProfile_CycleDetection(t *testing.T) {
	db := newTestContextProfile(t)

	cpA := &ContextProfile{ID: "ctx-a", Name: "profile-a"}
	cpB := &ContextProfile{ID: "ctx-b", Name: "profile-b"}
	if err := db.SaveContextProfile(cpA); err != nil {
		t.Fatalf("SaveContextProfile A failed: %v", err)
	}
	if err := db.SaveContextProfile(cpB); err != nil {
		t.Fatalf("SaveContextProfile B failed: %v", err)
	}

	linkA := &ContextProfileLink{
		ProfileID:       "ctx-a",
		Scope:           "global",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(linkA); err != nil {
		t.Fatalf("AddContextProfileLink A failed: %v", err)
	}

	linkBToA := &ContextProfileLink{
		ProfileID:       "ctx-b",
		ParentProfileID: strPtr("ctx-a"),
		Scope:           "",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(linkBToA); err != nil {
		t.Fatalf("AddContextProfileLink B->A failed: %v", err)
	}

	linkAToB := &ContextProfileLink{
		ProfileID:       "ctx-a",
		ParentProfileID: strPtr("ctx-b"),
		Scope:           "",
		PrecedenceOrder: 2,
	}
	if err := db.AddContextProfileLink(linkAToB); err != nil {
		t.Fatalf("AddContextProfileLink A->B failed: %v", err)
	}

	_, err := db.ResolveContextProfile("profile-a", DefaultMaxDepth)
	if err == nil {
		t.Fatal("expected cycle detection error, got nil")
	}
	if !strings.Contains(err.Error(), "cycle detected") {
		t.Errorf("expected 'cycle detected' in error, got: %v", err)
	}
}

func TestResolveContextProfile_MaxDepthExceeded(t *testing.T) {
	db := newTestContextProfile(t)

	for i := 0; i < 5; i++ {
		name := "depth-" + string(rune('a'+i))
		cp := &ContextProfile{
			ID:   "dp-" + string(rune('a'+i)),
			Name: name,
		}
		if err := db.SaveContextProfile(cp); err != nil {
			t.Fatalf("SaveContextProfile(%s) failed: %v", name, err)
		}
	}

	for i := 0; i < 4; i++ {
		childID := "dp-" + string(rune('a'+i))
		parentID := "dp-" + string(rune('a'+i+1))
		link := &ContextProfileLink{
			ProfileID:       childID,
			ParentProfileID: &parentID,
			PrecedenceOrder: 1,
		}
		if err := db.AddContextProfileLink(link); err != nil {
			t.Fatalf("AddContextProfileLink failed: %v", err)
		}
	}

	_, err := db.ResolveContextProfile("depth-a", 2)
	if err == nil {
		t.Fatal("expected max depth exceeded error, got nil")
	}
	if !strings.Contains(err.Error(), "exceeds maximum") {
		t.Errorf("expected 'exceeds maximum' in error, got: %v", err)
	}
}

func TestResolveContextProfile_ParentInheritance(t *testing.T) {
	db := newTestContextProfile(t)

	parent := &ContextProfile{ID: "par-1", Name: "parent-1", BaseScope: "global"}
	child := &ContextProfile{ID: "child-1", Name: "child-1", BaseScope: "session"}
	if err := db.SaveContextProfile(parent); err != nil {
		t.Fatalf("SaveContextProfile parent failed: %v", err)
	}
	if err := db.SaveContextProfile(child); err != nil {
		t.Fatalf("SaveContextProfile child failed: %v", err)
	}

	pLink := &ContextProfileLink{
		ProfileID:       "par-1",
		Scope:           "agent",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(pLink); err != nil {
		t.Fatalf("AddContextProfileLink parent failed: %v", err)
	}

	cLink := &ContextProfileLink{
		ProfileID:       "child-1",
		ParentProfileID: strPtr("par-1"),
		Scope:           "project",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(cLink); err != nil {
		t.Fatalf("AddContextProfileLink child failed: %v", err)
	}

	scopes, err := db.ResolveContextProfile("child-1", DefaultMaxDepth)
	if err != nil {
		t.Fatalf("ResolveContextProfile failed: %v", err)
	}

	expectedScopes := []string{"agent", "global", "project", "session"}
	if len(scopes) != len(expectedScopes) {
		t.Fatalf("expected %d resolved scopes, got %d", len(expectedScopes), len(scopes))
	}
	for i, s := range scopes {
		if s.Scope != expectedScopes[i] {
			t.Errorf("resolved scope[%d] = %q, want %q", i, s.Scope, expectedScopes[i])
		}
	}
}

func TestResolveContextProfile_NotFound(t *testing.T) {
	db := newTestContextProfile(t)

	_, err := db.ResolveContextProfile("does-not-exist", DefaultMaxDepth)
	if err == nil {
		t.Fatal("expected not-found error, got nil")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("expected 'not found' in error, got: %v", err)
	}
}

func TestIsolation_UnrelatedProfiles(t *testing.T) {
	db := newTestContextProfile(t)

	profileA := &ContextProfile{ID: "iso-a", Name: "profile-iso-a"}
	profileB := &ContextProfile{ID: "iso-b", Name: "profile-iso-b"}
	if err := db.SaveContextProfile(profileA); err != nil {
		t.Fatalf("SaveContextProfile A failed: %v", err)
	}
	if err := db.SaveContextProfile(profileB); err != nil {
		t.Fatalf("SaveContextProfile B failed: %v", err)
	}

	linkA := &ContextProfileLink{
		ProfileID:       "iso-a",
		Scope:           "global",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(linkA); err != nil {
		t.Fatalf("AddContextProfileLink A failed: %v", err)
	}

	linkB := &ContextProfileLink{
		ProfileID:       "iso-b",
		Scope:           "session",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(linkB); err != nil {
		t.Fatalf("AddContextProfileLink B failed: %v", err)
	}

	scopesA, err := db.ResolveContextProfile("profile-iso-a", DefaultMaxDepth)
	if err != nil {
		t.Fatalf("ResolveContextProfile A failed: %v", err)
	}
	scopesB, err := db.ResolveContextProfile("profile-iso-b", DefaultMaxDepth)
	if err != nil {
		t.Fatalf("ResolveContextProfile B failed: %v", err)
	}

	for _, s := range scopesA {
		if s.Scope == "session" {
			t.Error("profile A should not contain session scope (leak from profile B)")
		}
	}
	for _, s := range scopesB {
		if s.Scope == "global" {
			t.Error("profile B should not contain global scope (leak from profile A)")
		}
	}

	if len(scopesA) != 1 {
		t.Errorf("expected 1 scope for profile A, got %d", len(scopesA))
	}
	if len(scopesB) != 1 {
		t.Errorf("expected 1 scope for profile B, got %d", len(scopesB))
	}
}

func TestContextProfileFilterKeyRoundTrip(t *testing.T) {
	db := newTestContextProfile(t)

	cp := &ContextProfile{ID: "filter-1", Name: "filter-profile"}
	if err := db.SaveContextProfile(cp); err != nil {
		t.Fatalf("SaveContextProfile failed: %v", err)
	}

	link := &ContextProfileLink{
		ProfileID:       "filter-1",
		Scope:           "project",
		FilterKey:       "team",
		FilterValue:     "backend",
		PrecedenceOrder: 1,
	}
	if err := db.AddContextProfileLink(link); err != nil {
		t.Fatalf("AddContextProfileLink failed: %v", err)
	}

	links, err := db.ListContextProfileLinks("filter-profile")
	if err != nil {
		t.Fatalf("ListContextProfileLinks failed: %v", err)
	}
	if len(links) != 1 {
		t.Fatalf("expected 1 link, got %d", len(links))
	}
	if links[0].FilterKey != "team" || links[0].FilterValue != "backend" {
		t.Errorf("expected filter key=value 'team=backend', got '%s=%s'", links[0].FilterKey, links[0].FilterValue)
	}
}

func TestContextProfile_DeleteLink_NonExistentProfile(t *testing.T) {
	db := newTestContextProfile(t)

	err := db.RemoveContextProfileLink("ghost-profile", "global")
	if err == nil {
		t.Fatal("expected error for non-existent profile")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("expected 'not found' in error, got: %v", err)
	}
}
