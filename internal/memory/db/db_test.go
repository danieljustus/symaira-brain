package db

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func TestDBSchemaAndOperations(t *testing.T) {
	// Temporarily redirect home dir for testing to prevent polluting standard DB
	tempDir, err := os.MkdirTemp("", "symmemory-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	// Open temp DB
	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Verify database file was created inside temp directory
	expectedPath := filepath.Join(tempDir, ".local", "share", "symmemory", "default.db")
	if _, err := os.Stat(expectedPath); os.IsNotExist(err) {
		t.Errorf("database file was not created at expected path: %s", expectedPath)
	}

	// Create a test memory
	m := &Memory{
		ID:        "test-id-123",
		Content:   "User works on symaira.com",
		Scope:     "global",
		Metadata:  map[string]string{"source": "test"},
		Embedding: embeddingVector(1.0, 0.0, 0.0),
	}

	// Test Save
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save memory: %v", err)
	}

	// Test List
	mems, err := database.ListMemories("", 0, 100)
	if err != nil {
		t.Fatalf("failed to list memories: %v", err)
	}

	if len(mems) != 1 {
		t.Errorf("expected 1 memory, got %d", len(mems))
	} else {
		if mems[0].ID != "test-id-123" {
			t.Errorf("expected memory ID 'test-id-123', got '%s'", mems[0].ID)
		}
		if mems[0].Content != "User works on symaira.com" {
			t.Errorf("expected content 'User works on symaira.com', got '%s'", mems[0].Content)
		}
	}

	// Test Search (Vector Cosine Similarity check)
	query := embeddingVector(1.0, 0.0, 0.0) // Perfect match
	results, err := database.SearchMemoriesFiltered(query, "", "", 5, "", RankingWeights{RelevanceWeight: 1.0, RecencyWeight: 0, ImportanceWeight: 0, AccessReinforcementWeight: 0, RecencyHalfLife: 30, AccessHalfLife: 14})
	if err != nil {
		t.Fatalf("failed to search memories: %v", err)
	}

	if len(results) != 1 {
		t.Errorf("expected 1 search result, got %d", len(results))
	} else {
		if results[0].Score != 1.0 {
			t.Errorf("expected perfect similarity score 1.0, got %.4f", results[0].Score)
		}
	}

	// Test Session summaries
	if err := database.SaveSessionSummary("session-1", "Extracted test summary"); err != nil {
		t.Fatalf("failed to save session summary: %v", err)
	}

	sum, err := database.GetSessionSummary("session-1")
	if err != nil {
		t.Fatalf("failed to get summary: %v", err)
	}
	if sum != "Extracted test summary" {
		t.Errorf("expected summary 'Extracted test summary', got '%s'", sum)
	}

	// Test Delete
	if err := database.DeleteMemory("test-id-123"); err != nil {
		t.Fatalf("failed to delete memory: %v", err)
	}

	mems, err = database.ListMemories("", 0, 100)
	if err != nil {
		t.Fatalf("failed to list: %v", err)
	}
	if len(mems) != 0 {
		t.Errorf("expected empty database after deletion, got %d memories", len(mems))
	}
}

func TestMigrationsApplied(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-migrate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	var count int
	if err := database.conn.QueryRow(
		"SELECT COUNT(*) FROM schema_migrations",
	).Scan(&count); err != nil {
		t.Fatalf("schema_migrations table not created: %v", err)
	}
	if count == 0 {
		t.Errorf("expected at least one migration applied, got 0")
	}

	var version string
	if err := database.conn.QueryRow(
		"SELECT version FROM schema_migrations WHERE version = ?", "001_init",
	).Scan(&version); err != nil {
		t.Errorf("migration 001_init not recorded: %v", err)
	} else if version != "001_init" {
		t.Errorf("expected version '001_init', got '%s'", version)
	}
}

func TestMigrationsIdempotent(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-idem-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	db1, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("first open failed: %v", err)
	}
	_ = db1.Close()

	db2, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("second open failed: %v", err)
	}
	defer func() { _ = db2.Close() }()

	var count int
	if err := db2.conn.QueryRow(
		"SELECT COUNT(*) FROM schema_migrations",
	).Scan(&count); err != nil {
		t.Fatalf("failed to query migrations: %v", err)
	}
	entries, err := fs.ReadDir(migrationFS, "migrations")
	if err != nil {
		t.Fatalf("failed to read embedded migrations: %v", err)
	}
	if count != len(entries) {
		t.Errorf("expected %d migrations after two opens, got %d", len(entries), count)
	}
}

func TestCosineSimilarity(t *testing.T) {
	tests := []struct {
		name     string
		a        []float32
		b        []float32
		expected float32
	}{
		{
			name:     "Perfect alignment",
			a:        []float32{1.0, 0.0},
			b:        []float32{1.0, 0.0},
			expected: 1.0,
		},
		{
			name:     "Orthogonal vectors",
			a:        []float32{1.0, 0.0},
			b:        []float32{0.0, 1.0},
			expected: 0.0,
		},
		{
			name:     "Opposing vectors",
			a:        []float32{1.0, 0.0},
			b:        []float32{-1.0, 0.0},
			expected: -1.0,
		},
		{
			name:     "Empty vectors",
			a:        []float32{},
			b:        []float32{},
			expected: 0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			score := CosineSimilarity(tt.a, tt.b)
			// Check similarity with small float delta
			if mathAbs(score-tt.expected) > 1e-5 {
				t.Errorf("expected similarity %f, got %f", tt.expected, score)
			}
		})
	}
}

func mathAbs(f float32) float32 {
	if f < 0 {
		return -f
	}
	return f
}

func TestLSHConsistency(t *testing.T) {
	vec := make([]float32, EmbeddingDim)
	for i := range vec {
		vec[i] = float32(i) * 0.01
	}

	h1 := mustComputeLSH(vec)
	h2 := mustComputeLSH(vec)
	if h1 != h2 {
		t.Errorf("LSH not deterministic: %d vs %d", h1, h2)
	}
}

func TestLSHDifferentVectors(t *testing.T) {
	a := make([]float32, EmbeddingDim)
	b := make([]float32, EmbeddingDim)
	for i := range a {
		a[i] = float32(i) * 0.01
		b[i] = -float32(i) * 0.01
	}

	ha := mustComputeLSH(a)
	hb := mustComputeLSH(b)
	if ha == hb {
		t.Error("expected different LSH hashes for opposing vectors")
	}
}

func TestLSHEmptyVector(t *testing.T) {
	h := mustComputeLSH(nil)
	if h != 0 {
		t.Errorf("expected LSH 0 for nil vector, got %d", h)
	}
	h = mustComputeLSH([]float32{})
	if h != 0 {
		t.Errorf("expected LSH 0 for empty vector, got %d", h)
	}
}

func TestSearchWithLSHIndex(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-lsh-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Seed 100 memories with varying embeddings
	for i := 0; i < 100; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		emb[(i+37)%EmbeddingDim] = 0.5
		m := &Memory{
			ID:        fmt.Sprintf("lsh-mem-%d", i),
			Content:   fmt.Sprintf("Memory number %d with specific pattern", i),
			Scope:     "global",
			Embedding: emb,
		}
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to save memory %d: %v", i, err)
		}
	}

	// Search should only load a fraction of rows thanks to LSH pre-filter
	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0

	results, err := database.SearchMemories(queryVec, "", "global", 5)
	if err != nil {
		t.Fatalf("search failed: %v", err)
	}
	if len(results) == 0 {
		t.Error("expected at least one search result")
	}
}

// BenchmarkSearchWithLSH measures search performance with 1000 indexed memories.
func BenchmarkSearchWithLSH(b *testing.B) {
	tempDir, err := os.MkdirTemp("", "symmemory-bench-*")
	if err != nil {
		b.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		b.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Seed 1000 memories
	n := 1000
	for i := 0; i < n; i++ {
		emb := make([]float32, EmbeddingDim)
		emb[i%EmbeddingDim] = 1.0
		emb[(i+37)%EmbeddingDim] = 0.3
		m := &Memory{
			ID:        fmt.Sprintf("bench-%d", i),
			Content:   fmt.Sprintf("Benchmark memory entry %d", i),
			Scope:     "global",
			Embedding: emb,
		}
		if err := database.SaveMemory(m); err != nil {
			b.Fatalf("failed to seed: %v", err)
		}
	}

	queryVec := make([]float32, EmbeddingDim)
	queryVec[0] = 1.0
	queryVec[1] = 0.3

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := database.SearchMemories(queryVec, "", "global", 5)
		if err != nil {
			b.Fatalf("search failed: %v", err)
		}
	}
}

func TestDatabaseFilePermissions(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-perm-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	dbPath := filepath.Join(tempDir, ".local", "share", "symmemory", "default.db")

	// Check directory permissions
	dirPath := filepath.Dir(dbPath)
	dirInfo, err := os.Stat(dirPath)
	if err != nil {
		t.Fatalf("failed to stat directory: %v", err)
	}
	if dirInfo.Mode().Perm() != 0700 {
		t.Errorf("expected directory permissions 0700, got %o", dirInfo.Mode().Perm())
	}

	// Create a memory to ensure the DB file exists
	m := &Memory{
		ID:        "perm-test",
		Content:   "test content",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: embeddingVector(1.0),
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save memory: %v", err)
	}

	// Check database file permissions
	dbInfo, err := os.Stat(dbPath)
	if err != nil {
		t.Fatalf("failed to stat database file: %v", err)
	}
	if dbInfo.Mode().Perm() != 0600 {
		t.Errorf("expected database file permissions 0600, got %o", dbInfo.Mode().Perm())
	}
}

func TestGetMemoriesSinceFilter(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-since-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	oldTime := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)
	newTime := time.Date(2025, 6, 1, 0, 0, 0, 0, time.UTC)
	cutoff := time.Date(2025, 3, 1, 0, 0, 0, 0, time.UTC)

	for i := 0; i < 10; i++ {
		_, err := database.conn.Exec(
			`INSERT INTO memories (id, content, scope, metadata, embedding, embedding_dim, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session)
			 VALUES (?, ?, 'global', '{}', '[]', 0, 0, ?, ?, '', '', '', '')`,
			fmt.Sprintf("since-mem-%d", i), fmt.Sprintf("content %d", i), oldTime, oldTime,
		)
		if err != nil {
			t.Fatalf("failed to insert memory %d: %v", i, err)
		}
	}

	for i := 0; i < 3; i++ {
		_, err := database.conn.Exec(
			"UPDATE memories SET updated_at = ? WHERE id = ?",
			newTime, fmt.Sprintf("since-mem-%d", i),
		)
		if err != nil {
			t.Fatalf("failed to update memory %d: %v", i, err)
		}
	}

	results, err := database.GetMemoriesSince(cutoff)
	if err != nil {
		t.Fatalf("GetMemoriesSince failed: %v", err)
	}

	if len(results) != 3 {
		t.Errorf("expected 3 updated memories, got %d", len(results))
	}

	for _, m := range results {
		if !m.UpdatedAt.After(cutoff) {
			t.Errorf("returned memory %s with updated_at %v not after cutoff %v", m.ID, m.UpdatedAt, cutoff)
		}
	}
}

func TestFactExistsWithSpecialCharacters(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-fact-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	m := &Memory{
		ID:          "fact-test-1",
		Content:     "Test memory with special chars",
		Scope:       "global",
		ContentHash: "abc123",
		Metadata:    map[string]string{},
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save memory: %v", err)
	}

	exists, err := database.FactExists("abc123")
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if !exists {
		t.Error("expected FactExists to return true for existing hash")
	}

	exists, err = database.FactExists("nonexistent")
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if exists {
		t.Error("expected FactExists to return false for nonexistent hash")
	}

	exists, err = database.FactExists("abc%123")
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if exists {
		t.Error("expected FactExists to return false for hash with % character")
	}

	exists, err = database.FactExists("abc_123")
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if exists {
		t.Error("expected FactExists to return false for hash with _ character")
	}

	exists, err = database.FactExists("")
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if exists {
		t.Error("expected FactExists to return false for empty hash")
	}
}

func TestFactExistsAutoHash(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-autohash-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	content := "Alice prefers dark mode"
	autoHash := ComputeContentHash(content)

	m := &Memory{
		ID:      "auto-hash-1",
		Content: content,
		Scope:   "global",
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save memory: %v", err)
	}

	exists, err := database.FactExists(autoHash)
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if !exists {
		t.Error("expected FactExists to find auto-computed hash")
	}

	exists, err = database.FactExists(ComputeContentHash("different content"))
	if err != nil {
		t.Fatalf("FactExists failed: %v", err)
	}
	if exists {
		t.Error("expected FactExists to return false for different content hash")
	}
}

func TestDeleteMemoryEdgeCases(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-delete-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Test deleting non-existent ID returns no error
	err = database.DeleteMemory("nonexistent-id")
	if err != nil {
		t.Fatalf("DeleteMemory on non-existent ID should not error: %v", err)
	}

	// Test successful delete and verify gone
	m := &Memory{
		ID:        "delete-me",
		Content:   "to be deleted",
		Scope:     "global",
		Metadata:  map[string]string{},
		Embedding: embeddingVector(1.0, 0.0),
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("failed to save: %v", err)
	}

	got, err := database.GetMemory("delete-me")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected memory to exist before delete")
	}

	if err := database.DeleteMemory("delete-me"); err != nil {
		t.Fatalf("DeleteMemory failed: %v", err)
	}

	got, err = database.GetMemory("delete-me")
	if err != nil {
		t.Fatalf("GetMemory after delete failed: %v", err)
	}
	if got != nil {
		t.Error("expected memory to be nil after deletion")
	}

	// Verify ListMemories returns empty
	mems, err := database.ListMemories("", 0, 100)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	if len(mems) != 0 {
		t.Errorf("expected 0 memories after deletion, got %d", len(mems))
	}
}

func TestUpsertMemoryIfNewer(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-upsert-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	baseTime := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)

	// Test 1: Insert new memory (no existing row)
	m1 := &Memory{
		ID:        "upsert-1",
		Content:   "original content",
		Scope:     "global",
		Metadata:  map[string]string{"version": "1"},
		Embedding: embeddingVector(1.0, 0.0),
		UpdatedAt: baseTime,
	}
	inserted, err := database.UpsertMemoryIfNewer(m1)
	if err != nil {
		t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
	}
	if !inserted {
		t.Error("expected inserted=true for new memory")
	}

	// Verify it was saved
	got, err := database.GetMemory("upsert-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil || got.Content != "original content" {
		t.Errorf("expected content 'original content', got %v", got)
	}

	// Test 2: Update with newer timestamp (should succeed)
	m2 := &Memory{
		ID:        "upsert-1",
		Content:   "updated content",
		Scope:     "global",
		Metadata:  map[string]string{"version": "2"},
		Embedding: embeddingVector(1.0, 0.0),
		UpdatedAt: baseTime.Add(time.Hour), // 1 hour newer
	}
	updated, err := database.UpsertMemoryIfNewer(m2)
	if err != nil {
		t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
	}
	if !updated {
		t.Error("expected updated=true for newer timestamp")
	}

	got, err = database.GetMemory("upsert-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Content != "updated content" {
		t.Errorf("expected content 'updated content', got '%s'", got.Content)
	}

	// Test 3: Update with same timestamp (should skip)
	m3 := &Memory{
		ID:        "upsert-1",
		Content:   "same time content",
		Scope:     "global",
		Metadata:  map[string]string{"version": "3"},
		Embedding: embeddingVector(1.0, 0.0),
		UpdatedAt: baseTime.Add(time.Hour), // same as m2
	}
	skipped, err := database.UpsertMemoryIfNewer(m3)
	if err != nil {
		t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
	}
	if skipped {
		t.Error("expected skipped=false for same timestamp")
	}

	got, err = database.GetMemory("upsert-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Content != "updated content" {
		t.Errorf("expected content to remain 'updated content', got '%s'", got.Content)
	}

	// Test 4: Update with older timestamp (should skip)
	m4 := &Memory{
		ID:        "upsert-1",
		Content:   "older content",
		Scope:     "global",
		Metadata:  map[string]string{"version": "4"},
		Embedding: embeddingVector(1.0, 0.0),
		UpdatedAt: baseTime, // older than current
	}
	skipped, err = database.UpsertMemoryIfNewer(m4)
	if err != nil {
		t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
	}
	if skipped {
		t.Error("expected skipped=false for older timestamp")
	}

	got, err = database.GetMemory("upsert-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.Content != "updated content" {
		t.Errorf("expected content to remain 'updated content', got '%s'", got.Content)
	}
}

func TestListMemoriesLiteScopeAndPagination(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-lite-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Seed memories across scopes
	scopes := []string{"global", "global", "project", "project", "project", "agent"}
	for i, scope := range scopes {
		m := &Memory{
			ID:        fmt.Sprintf("lite-%d", i),
			Content:   fmt.Sprintf("content %d", i),
			Scope:     scope,
			Metadata:  map[string]string{},
			Embedding: embeddingVector(1.0, 0.0),
		}
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to save memory %d: %v", i, err)
		}
	}

	// Test 1: List all without scope filter
	all, err := database.ListMemoriesLite("", 0, 100)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(all) != 6 {
		t.Errorf("expected 6 memories total, got %d", len(all))
	}

	// Test 2: Filter by scope
	globalMems, err := database.ListMemoriesLite("global", 0, 100)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(globalMems) != 2 {
		t.Errorf("expected 2 global memories, got %d", len(globalMems))
	}
	for _, m := range globalMems {
		if m.Scope != "global" {
			t.Errorf("expected scope 'global', got '%s'", m.Scope)
		}
	}

	projectMems, err := database.ListMemoriesLite("project", 0, 100)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(projectMems) != 3 {
		t.Errorf("expected 3 project memories, got %d", len(projectMems))
	}

	// Test 3: Pagination - first page
	page1, err := database.ListMemoriesLite("", 0, 2)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(page1) != 2 {
		t.Errorf("expected 2 memories on first page, got %d", len(page1))
	}

	// Test 4: Pagination - second page
	page2, err := database.ListMemoriesLite("", 2, 2)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(page2) != 2 {
		t.Errorf("expected 2 memories on second page, got %d", len(page2))
	}

	// Ensure no overlap between pages
	ids1 := make(map[string]bool)
	for _, m := range page1 {
		ids1[m.ID] = true
	}
	for _, m := range page2 {
		if ids1[m.ID] {
			t.Errorf("memory %s appears on both pages", m.ID)
		}
	}

	// Test 5: Pagination - beyond available data
	pageEnd, err := database.ListMemoriesLite("", 100, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(pageEnd) != 0 {
		t.Errorf("expected 0 memories beyond available data, got %d", len(pageEnd))
	}

	// Test 6: Verify no embedding data in lite results
	for _, m := range all {
		if len(m.Embedding) != 0 {
			t.Errorf("ListMemoriesLite should not return embedding data for %s", m.ID)
		}
	}
}

func TestConsolidationStatusFiltering(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-status-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "test.db")

	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	m1 := &Memory{
		ID:                  "mem-raw",
		Content:             "This is raw memory",
		Scope:               "global",
		ConsolidationStatus: "raw",
		Metadata:            map[string]string{},
		Embedding:           embeddingVector(1.0, 0.0, 0.0),
	}
	m2 := &Memory{
		ID:                  "mem-consolidated",
		Content:             "This is consolidated memory",
		Scope:               "global",
		ConsolidationStatus: "consolidated",
		Metadata:            map[string]string{},
		Embedding:           embeddingVector(0.0, 1.0, 0.0),
	}
	m3 := &Memory{
		ID:                  "mem-archived",
		Content:             "This is archived memory",
		Scope:               "global",
		ConsolidationStatus: "archived",
		ConsolidatedIntoID:  "mem-consolidated",
		Metadata:            map[string]string{},
		Embedding:           embeddingVector(0.0, 0.0, 1.0),
	}

	if err := database.SaveMemory(m1); err != nil {
		t.Fatalf("failed to save raw memory: %v", err)
	}
	if err := database.SaveMemory(m2); err != nil {
		t.Fatalf("failed to save consolidated memory: %v", err)
	}
	if err := database.SaveMemory(m3); err != nil {
		t.Fatalf("failed to save archived memory: %v", err)
	}

	// 1. ListMemories should exclude 'archived'
	list, err := database.ListMemories("global", 0, 10)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	if len(list) != 2 {
		t.Errorf("expected 2 active memories in list, got %d", len(list))
	}
	for _, m := range list {
		if m.ConsolidationStatus == "archived" {
			t.Errorf("archived memory was returned in ListMemories")
		}
	}

	// 2. SearchMemories should exclude 'archived'
	searchVal := embeddingVector(0.0, 0.0, 1.0) // perfect alignment with mem-archived
	searchResults, err := database.SearchMemories(searchVal, "", "global", 3)
	if err != nil {
		t.Fatalf("SearchMemories failed: %v", err)
	}
	for _, res := range searchResults {
		if res.Memory.ID == "mem-archived" {
			t.Errorf("archived memory was returned in SearchMemories")
		}
	}

	// 3. GetMemory directly should retrieve any status
	archived, err := database.GetMemory("mem-archived")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if archived == nil {
		t.Fatalf("expected to retrieve archived memory directly, got nil")
	}
	if archived.ConsolidationStatus != "archived" {
		t.Errorf("expected status 'archived', got '%s'", archived.ConsolidationStatus)
	}
	if archived.ConsolidatedIntoID != "mem-consolidated" {
		t.Errorf("expected consolidated_into_id 'mem-consolidated', got '%s'", archived.ConsolidatedIntoID)
	}

	// 4. GetMemoriesSince should include 'archived'
	sinceResults, err := database.GetMemoriesSince(time.Now().UTC().Add(-1 * time.Hour))
	if err != nil {
		t.Fatalf("GetMemoriesSince failed: %v", err)
	}
	foundArchived := false
	for _, m := range sinceResults {
		if m.ID == "mem-archived" {
			foundArchived = true
		}
	}
	if !foundArchived {
		t.Errorf("GetMemoriesSince did not return archived memory")
	}
}

func TestSetMemoryEmbedding(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-setemb-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	m := &Memory{
		ID:        "setemb-1",
		Content:   "test memory content",
		Scope:     "global",
		Metadata:  map[string]string{"source": "test"},
		Embedding: nil,
		CreatedAt: time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC),
		UpdatedAt: time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC),
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	got, err := database.GetMemory("setemb-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if len(got.Embedding) != 0 {
		t.Fatalf("expected no embedding initially, got %d dims", len(got.Embedding))
	}

	// A dimension mismatch is a handled error, never a silent truncation.
	if err := database.SetMemoryEmbedding("setemb-1", []float32{0.5, 0.3, 0.8, 0.1, 0.9}, "hash-fallback", "", ""); err == nil {
		t.Fatal("expected dimension mismatch error for 5-dim embedding, got nil")
	}

	newEmb := embeddingVector(0.5, 0.3, 0.8, 0.1, 0.9)
	if err := database.SetMemoryEmbedding("setemb-1", newEmb, "hash-fallback", "", ""); err != nil {
		t.Fatalf("SetMemoryEmbedding failed: %v", err)
	}

	got, err = database.GetMemory("setemb-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if len(got.Embedding) != EmbeddingDim {
		t.Fatalf("expected %d-dim embedding, got %d", EmbeddingDim, len(got.Embedding))
	}
	for i, v := range got.Embedding {
		if v != newEmb[i] {
			t.Errorf("embedding[%d] = %f, want %f", i, v, newEmb[i])
		}
	}

	if got.CreatedAt.Year() != 2025 {
		t.Errorf("created_at should be preserved from original, got %v", got.CreatedAt)
	}
	if got.Content != "test memory content" {
		t.Errorf("content should be preserved, got %q", got.Content)
	}
}

func TestSyncedMemorySearchableAfterEmbeddingBackfill(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-sync-search-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	syncTime := time.Date(2025, 6, 1, 12, 0, 0, 0, time.UTC)

	syncedMemories := []*Memory{
		{
			ID:        "sync-1",
			Content:   "Alice prefers dark mode in all applications",
			Scope:     "agent",
			Metadata:  map[string]string{"source": "remote-sync"},
			Embedding: nil,
			CreatedAt: syncTime,
			UpdatedAt: syncTime,
		},
		{
			ID:        "sync-2",
			Content:   "Bob uses light theme exclusively",
			Scope:     "agent",
			Metadata:  map[string]string{"source": "remote-sync"},
			Embedding: nil,
			CreatedAt: syncTime,
			UpdatedAt: syncTime,
		},
	}

	for _, m := range syncedMemories {
		ok, err := database.UpsertMemoryIfNewer(m)
		if err != nil {
			t.Fatalf("UpsertMemoryIfNewer failed: %v", err)
		}
		if !ok {
			t.Fatalf("expected upsert to succeed for %s", m.ID)
		}
	}

	for _, m := range syncedMemories {
		got, err := database.GetMemory(m.ID)
		if err != nil {
			t.Fatalf("GetMemory failed: %v", err)
		}
		if len(got.Embedding) != 0 {
			t.Fatalf("expected no embedding before backfill for %s", m.ID)
		}
	}

	emb := extractor.GenerateLocalHashVector("Alice prefers dark mode in all applications", 768)
	if err := database.SetMemoryEmbedding("sync-1", emb, "hash-fallback", "", ""); err != nil {
		t.Fatalf("SetMemoryEmbedding failed: %v", err)
	}

	emb2 := extractor.GenerateLocalHashVector("Bob uses light theme exclusively", 768)
	if err := database.SetMemoryEmbedding("sync-2", emb2, "hash-fallback", "", ""); err != nil {
		t.Fatalf("SetMemoryEmbedding failed: %v", err)
	}

	for _, m := range syncedMemories {
		got, err := database.GetMemory(m.ID)
		if err != nil {
			t.Fatalf("GetMemory failed: %v", err)
		}
		if len(got.Embedding) == 0 {
			t.Errorf("expected non-empty embedding after backfill for %s", m.ID)
		}
		if len(got.Embedding) != 768 {
			t.Errorf("expected 768-dim embedding for %s, got %d", m.ID, len(got.Embedding))
		}
	}

	got, err := database.GetMemory("sync-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got.CreatedAt.Year() != 2025 || got.CreatedAt.Month() != 6 {
		t.Errorf("created_at should be preserved from sync, got %v", got.CreatedAt)
	}
	if got.Content != "Alice prefers dark mode in all applications" {
		t.Errorf("content should be preserved from sync, got %q", got.Content)
	}

	queryVec := extractor.GenerateLocalHashVector("Alice prefers dark mode in all applications", 768)
	results, err := database.SearchMemories(queryVec, "hash-fallback", "", 10)
	if err != nil {
		t.Fatalf("SearchMemories failed: %v", err)
	}

	found := false
	for _, r := range results {
		if r.Memory.ID == "sync-1" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("synced memory not found via semantic search (LSH may not match for local hash vectors; embedding is %d-dim)", len(emb))
	}
}

func TestSearchMemoriesFiltersEmbeddingSourceAtCandidateQuery(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Use a single shared embedding so every row lands in the same LSH bucket.
	sharedVec := make([]float32, EmbeddingDim)
	sharedVec[0] = 1.0

	now := time.Now().UTC()
	older := now.Add(-2 * time.Hour)

	// Insert 2000 rows with a different embedding source than the query.
	// They share the same LSH hash as the target row, so before the SQL filter
	// they would consume the entire candidate budget and push the older target
	// row out of the result set.
	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatalf("BeginTransaction failed: %v", err)
	}
	for i := 0; i < 2000; i++ {
		m := &Memory{
			ID:              fmt.Sprintf("ollama-%d", i),
			Content:         fmt.Sprintf("ollama content %d", i),
			Scope:           "global",
			Embedding:       sharedVec,
			EmbeddingSource: "ollama",
			EmbeddingModel:  "ollama-model",
			CreatedAt:       now.Add(-time.Duration(i) * time.Second),
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			_ = tx.Rollback()
			t.Fatalf("SaveMemoryTx failed: %v", err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("Commit failed: %v", err)
	}

	// Insert the target row with the query source, older than the ollama rows.
	target := &Memory{
		ID:              "target-hash",
		Content:         "target hash fallback content",
		Scope:           "global",
		Embedding:       sharedVec,
		EmbeddingSource: "hash-fallback",
		EmbeddingModel:  "hash-model",
		CreatedAt:       older,
	}
	if err := database.SaveMemory(target); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	results, err := database.SearchMemories(sharedVec, "hash-fallback", "global", 10)
	if err != nil {
		t.Fatalf("SearchMemories failed: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Memory.ID != "target-hash" {
		t.Errorf("expected target-hash, got %s", results[0].Memory.ID)
	}
	if results[0].Memory.EmbeddingSource != "hash-fallback" {
		t.Errorf("expected embedding_source hash-fallback, got %s", results[0].Memory.EmbeddingSource)
	}
}

// embeddingVector builds a valid EmbeddingDim-sized vector with the given
// leading values (dimension mismatches are now a handled error).
func embeddingVector(leading ...float32) []float32 {
	v := make([]float32, EmbeddingDim)
	copy(v, leading)
	return v
}
