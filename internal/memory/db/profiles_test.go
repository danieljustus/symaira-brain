package db

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// newTestDB creates a temporary database for profile tests.
func newTestDB(t *testing.T) *DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-profile-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func TestSaveProfile_Insert(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:          "prof-1",
		Name:        "Alice",
		Type:        "agent",
		Role:        "admin",
		Description: "Test agent",
		Metadata:    map[string]any{"team": "alpha", "level": float64(3)},
	}

	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile insert failed: %v", err)
	}

	if p.CreatedAt.IsZero() {
		t.Error("expected CreatedAt to be set automatically")
	}
	if p.UpdatedAt.IsZero() {
		t.Error("expected UpdatedAt to be set automatically")
	}
}

func TestSaveProfile_Upsert(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:          "prof-upsert",
		Name:        "Bob",
		Type:        "agent",
		Role:        "readonly",
		Description: "original",
		Metadata:    map[string]any{"v": float64(1)},
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("initial SaveProfile failed: %v", err)
	}
	originalCreated := p.CreatedAt

	// Wait a moment so UpdatedAt differs.
	time.Sleep(10 * time.Millisecond)

	p.Role = "readwrite"
	p.Description = "updated"
	p.Metadata = map[string]any{"v": float64(2)}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("upsert SaveProfile failed: %v", err)
	}

	got, err := db.GetProfileByName("Bob")
	if err != nil {
		t.Fatalf("GetProfileByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected profile after upsert, got nil")
	}
	if got.Role != "readwrite" {
		t.Errorf("expected role 'readwrite', got %q", got.Role)
	}
	if got.Description != "updated" {
		t.Errorf("expected description 'updated', got %q", got.Description)
	}
	if !got.CreatedAt.Equal(originalCreated) {
		t.Errorf("CreatedAt should be preserved on upsert, got %v vs %v", got.CreatedAt, originalCreated)
	}
}

func TestSaveProfile_PreservesExistingTimestamps(t *testing.T) {
	db := newTestDB(t)

	fixed := time.Date(2025, 6, 1, 12, 0, 0, 0, time.UTC)
	p := &Profile{
		ID:        "prof-ts",
		Name:      "Timestamp",
		Type:      "human",
		Role:      "readwrite",
		CreatedAt: fixed,
		UpdatedAt: fixed,
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}
	if !p.CreatedAt.Equal(fixed) {
		t.Errorf("CreatedAt should be preserved, got %v", p.CreatedAt)
	}
	if !p.UpdatedAt.Equal(fixed) {
		t.Errorf("UpdatedAt should be preserved, got %v", p.UpdatedAt)
	}
}

func TestSaveProfile_MarshalError(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:       "prof-bad-meta",
		Name:     "BadMeta",
		Type:     "agent",
		Role:     "readonly",
		Metadata: map[string]any{"ch": make(chan int)},
	}
	err := db.SaveProfile(p)
	if err == nil {
		t.Fatal("expected marshal error for unmarshalable metadata, got nil")
	}
}

func TestGetProfileByName_Found(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:          "prof-get",
		Name:        "Charlie",
		Type:        "agent",
		Role:        "admin",
		Description: "desc",
		Metadata:    map[string]any{"key": "value"},
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}

	got, err := db.GetProfileByName("Charlie")
	if err != nil {
		t.Fatalf("GetProfileByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected profile, got nil")
	}
	if got.ID != "prof-get" {
		t.Errorf("expected ID 'prof-get', got %q", got.ID)
	}
	if got.Name != "Charlie" {
		t.Errorf("expected Name 'Charlie', got %q", got.Name)
	}
	if got.Role != "admin" {
		t.Errorf("expected Role 'admin', got %q", got.Role)
	}
	if got.Metadata["key"] != "value" {
		t.Errorf("expected metadata key=value, got %v", got.Metadata)
	}
}

func TestGetProfileByName_CaseInsensitive(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:   "prof-case",
		Name: "DeltaAgent",
		Type: "agent",
		Role: "readonly",
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}

	for _, variant := range []string{"deltaagent", "DELTAAGENT", "DeltaAgent", "dElTaAgEnT"} {
		got, err := db.GetProfileByName(variant)
		if err != nil {
			t.Fatalf("GetProfileByName(%q) failed: %v", variant, err)
		}
		if got == nil {
			t.Errorf("GetProfileByName(%q) returned nil, expected profile", variant)
		}
	}
}

func TestGetProfileByName_NotFound(t *testing.T) {
	db := newTestDB(t)

	got, err := db.GetProfileByName("nonexistent")
	if err != nil {
		t.Fatalf("GetProfileByName should return nil error for missing profile, got: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil for missing profile, got %+v", got)
	}
}

func TestGetProfileByName_InvalidMetadataJSON(t *testing.T) {
	db := newTestDB(t)

	// Insert a profile with invalid JSON metadata directly via SQL.
	_, err := db.conn.Exec(
		`INSERT INTO profiles (id, name, type, role, description, metadata, created_at, updated_at)
		 VALUES ('prof-bad-json', 'BadJSON', 'agent', 'readonly', '', 'not-valid-json', ?, ?)`,
		time.Now().UTC(), time.Now().UTC(),
	)
	if err != nil {
		t.Fatalf("failed to insert bad-json profile: %v", err)
	}

	_, err = db.GetProfileByName("BadJSON")
	if err == nil {
		t.Fatal("expected unmarshal error for invalid metadata JSON, got nil")
	}
}

func TestListProfiles_Empty(t *testing.T) {
	db := newTestDB(t)

	profiles, err := db.ListProfiles()
	if err != nil {
		t.Fatalf("ListProfiles failed: %v", err)
	}
	if len(profiles) != 0 {
		t.Errorf("expected 0 profiles, got %d", len(profiles))
	}
}

func TestListProfiles_Multiple(t *testing.T) {
	db := newTestDB(t)

	names := []string{"Zara", "alice", "Bob"}
	for i, name := range names {
		p := &Profile{
			ID:       filepath.Base(name),
			Name:     name,
			Type:     "agent",
			Role:     "readonly",
			Metadata: map[string]any{"idx": float64(i)},
		}
		if err := db.SaveProfile(p); err != nil {
			t.Fatalf("SaveProfile(%s) failed: %v", name, err)
		}
	}

	profiles, err := db.ListProfiles()
	if err != nil {
		t.Fatalf("ListProfiles failed: %v", err)
	}
	if len(profiles) != 3 {
		t.Fatalf("expected 3 profiles, got %d", len(profiles))
	}

	// Verify case-insensitive ordering: alice, Bob, Zara
	expectedOrder := []string{"alice", "Bob", "Zara"}
	for i, p := range profiles {
		if p.Name != expectedOrder[i] {
			t.Errorf("profile[%d] name = %q, want %q", i, p.Name, expectedOrder[i])
		}
	}

	// Verify metadata round-trip.
	for _, p := range profiles {
		if p.Metadata == nil {
			t.Errorf("expected non-nil metadata for %s", p.Name)
		}
	}
}

func TestListProfiles_InvalidMetadataJSON(t *testing.T) {
	db := newTestDB(t)

	_, err := db.conn.Exec(
		`INSERT INTO profiles (id, name, type, role, description, metadata, created_at, updated_at)
		 VALUES ('prof-list-bad', 'ListBad', 'agent', 'readonly', '', '{broken', ?, ?)`,
		time.Now().UTC(), time.Now().UTC(),
	)
	if err != nil {
		t.Fatalf("failed to insert bad-json profile: %v", err)
	}

	_, err = db.ListProfiles()
	if err == nil {
		t.Fatal("expected unmarshal error from ListProfiles with invalid JSON, got nil")
	}
}

func TestDeleteProfile_Success(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:   "prof-del",
		Name: "ToDelete",
		Type: "agent",
		Role: "readonly",
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}

	if err := db.DeleteProfile("ToDelete"); err != nil {
		t.Fatalf("DeleteProfile failed: %v", err)
	}

	got, err := db.GetProfileByName("ToDelete")
	if err != nil {
		t.Fatalf("GetProfileByName after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected profile to be deleted, got non-nil")
	}
}

func TestDeleteProfile_CaseInsensitive(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:   "prof-del-case",
		Name: "CaseSensitive",
		Type: "agent",
		Role: "readonly",
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}

	if err := db.DeleteProfile("CASESENSITIVE"); err != nil {
		t.Fatalf("DeleteProfile (case-insensitive) failed: %v", err)
	}

	got, err := db.GetProfileByName("CaseSensitive")
	if err != nil {
		t.Fatalf("GetProfileByName after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected profile to be deleted via case-insensitive match")
	}
}

func TestDeleteProfile_NonExistent(t *testing.T) {
	db := newTestDB(t)

	// Deleting a non-existent profile should not error.
	if err := db.DeleteProfile("ghost"); err != nil {
		t.Fatalf("DeleteProfile on non-existent name should not error, got: %v", err)
	}
}

func TestProfileMetadataRoundTrip(t *testing.T) {
	db := newTestDB(t)

	meta := map[string]any{
		"string": "hello",
		"number": float64(42),
		"bool":   true,
		"null":   nil,
		"nested": map[string]any{"inner": "value"},
		"array":  []any{float64(1), "two", false},
	}

	p := &Profile{
		ID:       "prof-meta-rt",
		Name:     "MetaRoundTrip",
		Type:     "agent",
		Role:     "admin",
		Metadata: meta,
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile failed: %v", err)
	}

	got, err := db.GetProfileByName("MetaRoundTrip")
	if err != nil {
		t.Fatalf("GetProfileByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected profile, got nil")
	}

	if got.Metadata["string"] != "hello" {
		t.Errorf("metadata string = %v, want 'hello'", got.Metadata["string"])
	}
	if got.Metadata["number"] != float64(42) {
		t.Errorf("metadata number = %v, want 42", got.Metadata["number"])
	}
	if got.Metadata["bool"] != true {
		t.Errorf("metadata bool = %v, want true", got.Metadata["bool"])
	}
	if got.Metadata["null"] != nil {
		t.Errorf("metadata null = %v, want nil", got.Metadata["null"])
	}
	nested, ok := got.Metadata["nested"].(map[string]any)
	if !ok {
		t.Fatalf("metadata nested is not a map, got %T", got.Metadata["nested"])
	}
	if nested["inner"] != "value" {
		t.Errorf("metadata nested.inner = %v, want 'value'", nested["inner"])
	}
	arr, ok := got.Metadata["array"].([]any)
	if !ok {
		t.Fatalf("metadata array is not a slice, got %T", got.Metadata["array"])
	}
	if len(arr) != 3 {
		t.Errorf("metadata array length = %d, want 3", len(arr))
	}
}

func TestProfileNilMetadata(t *testing.T) {
	db := newTestDB(t)

	p := &Profile{
		ID:       "prof-nil-meta",
		Name:     "NilMeta",
		Type:     "agent",
		Role:     "readonly",
		Metadata: nil,
	}
	if err := db.SaveProfile(p); err != nil {
		t.Fatalf("SaveProfile with nil metadata failed: %v", err)
	}

	got, err := db.GetProfileByName("NilMeta")
	if err != nil {
		t.Fatalf("GetProfileByName failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected profile, got nil")
	}
	// json.Unmarshal of "null" into map[string]any yields nil, which is acceptable.
}
