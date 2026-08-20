package db

import (
	"testing"
	"time"
)

func TestSaveMemory_WorkingTier(t *testing.T) {
	// Given
	database := newTestDB(t)
	expires := time.Now().UTC().Add(24 * time.Hour)
	m := &Memory{
		ID:        "working-1",
		Content:   "Current task: implementing feature X",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &expires,
		Metadata:  map[string]string{"source_type": "direct"},
	}

	// When
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	// Then
	got, err := database.GetMemory("working-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Fatal("GetMemory returned nil")
	}
	if got.Tier != "working" {
		t.Errorf("expected Tier=working, got %q", got.Tier)
	}
	if got.ExpiresAt == nil {
		t.Fatal("expected ExpiresAt to be set")
	}
	if got.ExpiresAt.Sub(expires) > time.Second || expires.Sub(*got.ExpiresAt) > time.Second {
		t.Errorf("ExpiresAt mismatch: got %v, want %v", *got.ExpiresAt, expires)
	}
}

func TestSaveMemory_DefaultTier(t *testing.T) {
	// Given
	database := newTestDB(t)
	m := &Memory{
		ID:       "default-1",
		Content:  "Alice prefers dark mode",
		Scope:    "global",
		Metadata: map[string]string{"source_type": "direct"},
	}

	// When
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	// Then
	got, err := database.GetMemory("default-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Fatal("GetMemory returned nil")
	}
	if got.Tier != "long_term" {
		t.Errorf("expected Tier=long_term, got %q", got.Tier)
	}
	if got.ExpiresAt != nil {
		t.Errorf("expected ExpiresAt=nil, got %v", *got.ExpiresAt)
	}
}

func TestListMemories_ExcludesExpiredWorking(t *testing.T) {
	// Given
	database := newTestDB(t)

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	if err := database.SaveMemory(&Memory{
		ID:        "working-active",
		Content:   "Active working memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (active) failed: %v", err)
	}

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	if err := database.SaveMemory(&Memory{
		ID:        "working-expired",
		Content:   "Expired working memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (expired) failed: %v", err)
	}

	if err := database.SaveMemory(&Memory{
		ID:       "long-term-1",
		Content:  "Long-term fact",
		Scope:    "project",
		Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (long-term) failed: %v", err)
	}

	// When
	memories, err := database.ListMemories("project", 0, 100)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}

	// Then
	ids := make(map[string]bool)
	for _, m := range memories {
		ids[m.ID] = true
	}
	if !ids["working-active"] {
		t.Error("active working memory should be included in ListMemories")
	}
	if ids["working-expired"] {
		t.Error("expired working memory should be excluded from ListMemories")
	}
	if !ids["long-term-1"] {
		t.Error("long-term memory should be included in ListMemories")
	}
}

func TestGetWorkingMemories(t *testing.T) {
	// Given
	database := newTestDB(t)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	if err := database.SaveMemory(&Memory{
		ID:        "w1",
		Content:   "Working task 1",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (working) failed: %v", err)
	}
	if err := database.SaveMemory(&Memory{
		ID:       "lt1",
		Content:  "Long-term fact",
		Scope:    "project",
		Tier:     "long_term",
		Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (long-term) failed: %v", err)
	}

	// When
	memories, err := database.GetWorkingMemories("project", 10)
	if err != nil {
		t.Fatalf("GetWorkingMemories failed: %v", err)
	}

	// Then
	if len(memories) != 1 {
		t.Fatalf("expected 1 working memory, got %d", len(memories))
	}
	if memories[0].ID != "w1" {
		t.Errorf("expected ID=w1, got %q", memories[0].ID)
	}
}

func TestGetExpiredWorkingMemories(t *testing.T) {
	// Given
	database := newTestDB(t)
	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	if err := database.SaveMemory(&Memory{
		ID:        "expired-1",
		Content:   "Expired task",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (expired) failed: %v", err)
	}
	if err := database.SaveMemory(&Memory{
		ID:        "active-1",
		Content:   "Active task",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (active) failed: %v", err)
	}

	// When
	memories, err := database.GetExpiredWorkingMemories()
	if err != nil {
		t.Fatalf("GetExpiredWorkingMemories failed: %v", err)
	}

	// Then
	if len(memories) != 1 {
		t.Fatalf("expected 1 expired memory, got %d", len(memories))
	}
	if memories[0].ID != "expired-1" {
		t.Errorf("expected ID=expired-1, got %q", memories[0].ID)
	}
}

func TestEvictExpiredWorkingMemories(t *testing.T) {
	// Given
	database := newTestDB(t)
	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	if err := database.SaveMemory(&Memory{
		ID:        "expired-1",
		Content:   "Expired task",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (expired) failed: %v", err)
	}
	if err := database.SaveMemory(&Memory{
		ID:        "active-1",
		Content:   "Active task",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (active) failed: %v", err)
	}

	// When
	count, err := database.EvictExpiredWorkingMemories()
	if err != nil {
		t.Fatalf("EvictExpiredWorkingMemories failed: %v", err)
	}

	// Then
	if count != 1 {
		t.Errorf("expected 1 eviction, got %d", count)
	}
	got, err := database.GetMemory("expired-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Error("expired working memory should be deleted after eviction")
	}
	gotActive, err := database.GetMemory("active-1")
	if err != nil {
		t.Fatalf("GetMemory (active) failed: %v", err)
	}
	if gotActive == nil {
		t.Error("active working memory should remain after eviction")
	}
}

func TestSetMemoryTier(t *testing.T) {
	// Given
	database := newTestDB(t)
	if err := database.SaveMemory(&Memory{
		ID:       "mem-1",
		Content:  "A memory",
		Scope:    "project",
		Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	got, err := database.GetMemory("mem-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Tier != "long_term" {
		t.Errorf("expected default Tier=long_term, got %q", got.Tier)
	}

	// When — promote to working with expiry
	futureExpiry := time.Now().UTC().Add(1 * time.Hour)
	if err := database.SetMemoryTier("mem-1", "working", &futureExpiry); err != nil {
		t.Fatalf("SetMemoryTier (working) failed: %v", err)
	}

	// Then
	got, err = database.GetMemory("mem-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Tier != "working" {
		t.Errorf("expected Tier=working, got %q", got.Tier)
	}
	if got.ExpiresAt == nil {
		t.Fatal("expected ExpiresAt to be set")
	}

	// When — demote back to long_term (clears expiry)
	if err := database.SetMemoryTier("mem-1", "long_term", nil); err != nil {
		t.Fatalf("SetMemoryTier (long_term) failed: %v", err)
	}

	// Then
	got, err = database.GetMemory("mem-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Tier != "long_term" {
		t.Errorf("expected Tier=long_term after demotion, got %q", got.Tier)
	}
	if got.ExpiresAt != nil {
		t.Errorf("expected ExpiresAt=nil after demotion, got %v", *got.ExpiresAt)
	}
}

func TestSearchExcludesExpiredWorking(t *testing.T) {
	// Given
	database := newTestDB(t)
	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	emb := embeddingVector(1.0)

	if err := database.SaveMemory(&Memory{
		ID:              "expired-emb",
		Content:         "Expired embedded working memory about databases",
		Scope:           "",
		Tier:            "working",
		ExpiresAt:       &pastExpiry,
		Embedding:       emb,
		EmbeddingSource: "test",
		Metadata:        map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (expired) failed: %v", err)
	}

	if err := database.SaveMemory(&Memory{
		ID:              "active-emb",
		Content:         "Active embedded working memory about databases",
		Scope:           "",
		Tier:            "working",
		ExpiresAt:       &futureExpiry,
		Embedding:       emb,
		EmbeddingSource: "test",
		Metadata:        map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (active) failed: %v", err)
	}

	// When
	results, err := database.SearchMemories(emb, "test", "", 10)
	if err != nil {
		t.Fatalf("SearchMemories failed: %v", err)
	}

	// Then
	for _, r := range results {
		if r.Memory.ID == "expired-emb" {
			t.Error("expired working memory should not appear in search results")
		}
	}
	found := false
	for _, r := range results {
		if r.Memory.ID == "active-emb" {
			found = true
			break
		}
	}
	if !found {
		t.Error("active working memory should appear in search results")
	}
}

func TestListMemoriesLite_ExcludesExpiredWorking(t *testing.T) {
	// Given
	database := newTestDB(t)
	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	if err := database.SaveMemory(&Memory{
		ID:        "lite-expired",
		Content:   "Expired lite memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (expired) failed: %v", err)
	}
	if err := database.SaveMemory(&Memory{
		ID:        "lite-active",
		Content:   "Active lite memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory (active) failed: %v", err)
	}

	// When
	memories, err := database.ListMemoriesLite("project", 0, 100)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}

	// Then
	ids := make(map[string]bool)
	for _, m := range memories {
		ids[m.ID] = true
	}
	if !ids["lite-active"] {
		t.Error("active lite memory should be included in ListMemoriesLite")
	}
	if ids["lite-expired"] {
		t.Error("expired lite memory should be excluded from ListMemoriesLite")
	}
}
