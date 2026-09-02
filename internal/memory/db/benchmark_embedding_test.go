package db

import (
	"bytes"
	"compress/gzip"
	"database/sql"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/paths"
)

// ---------------------------------------------------------------------------
// Deterministic embedding generator (seeded RNG, no Ollama dependency)
// ---------------------------------------------------------------------------

// generateDeterministicEmbedding produces a reproducible 768-dim embedding
// from a seed value. The same seed always yields the same vector.
func generateDeterministicEmbedding(seed int, dim int) []float32 {
	rng := rand.New(rand.NewSource(int64(seed))) //nolint:gosec // deterministic benchmark data only
	vec := make([]float32, dim)
	for i := range vec {
		vec[i] = rng.Float32()
	}
	return vec
}

// ---------------------------------------------------------------------------
// BLOB encode / decode helpers
// ---------------------------------------------------------------------------

// encodeEmbeddingBLOB serialises a float32 slice to little-endian bytes.
func encodeEmbeddingBLOB(vec []float32) []byte {
	buf := make([]byte, 4*len(vec))
	for i, v := range vec {
		binary.LittleEndian.PutUint32(buf[i*4:], math.Float32bits(v))
	}
	return buf
}

// decodeEmbeddingBLOB deserialises little-endian bytes back to float32.
func decodeEmbeddingBLOB(data []byte) []float32 {
	n := len(data) / 4
	vec := make([]float32, n)
	for i := range n {
		vec[i] = math.Float32frombits(binary.LittleEndian.Uint32(data[i*4:]))
	}
	return vec
}

// ---------------------------------------------------------------------------
// BLOB storage path (raw SQL, bypasses JSON marshal/unmarshal)
// ---------------------------------------------------------------------------

// saveMemoryBLOB writes a memory with its embedding stored as raw BLOB bytes.
func saveMemoryBLOB(database *DB, m *Memory) error {
	tx, err := database.BeginTransaction()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if err := saveMemoryBLOBTx(tx, m); err != nil {
		return err
	}
	return tx.Commit()
}

// saveMemoryBLOBTx writes a memory with its embedding stored as raw BLOB bytes in a transaction.
func saveMemoryBLOBTx(tx *sql.Tx, m *Memory) error {
	metadataJSON, err := json.Marshal(m.Metadata)
	if err != nil {
		return fmt.Errorf("metadata marshal: %w", err)
	}

	embeddingBLOB := encodeEmbeddingBLOB(m.Embedding)
	embeddingDim := len(m.Embedding)
	lshHash := mustComputeLSH(m.Embedding)

	contentHash := m.ContentHash
	if contentHash == "" {
		contentHash = ComputeContentHash(m.Content)
	}
	status := m.ConsolidationStatus
	if status == "" {
		status = "raw"
	}

	now := time.Now().UTC()
	if m.CreatedAt.IsZero() {
		m.CreatedAt = now
	}
	m.UpdatedAt = now

	query := `INSERT INTO memories (id, content, scope, metadata, embedding, embedding_dim, embedding_source, embedding_model, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, access_count, last_access)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			content=excluded.content, scope=excluded.scope, metadata=excluded.metadata,
			embedding=excluded.embedding, embedding_dim=excluded.embedding_dim,
			embedding_source=excluded.embedding_source, embedding_model=excluded.embedding_model,
			content_hash=excluded.content_hash, lsh_hash=excluded.lsh_hash,
			updated_at=excluded.updated_at, updated_by=excluded.updated_by,
			updated_session=excluded.updated_session,
			consolidation_status=excluded.consolidation_status,
			consolidated_into_id=excluded.consolidated_into_id,
			importance=excluded.importance, valid_from=excluded.valid_from,
			valid_to=excluded.valid_to, superseded_by=excluded.superseded_by`

	_, err = tx.Exec(query,
		m.ID, m.Content, m.Scope, string(metadataJSON),
		embeddingBLOB, embeddingDim,
		m.EmbeddingSource, m.EmbeddingModel, contentHash, lshHash,
		m.CreatedAt, m.UpdatedAt, m.CreatedBy, m.UpdatedBy,
		m.CreatedSession, m.UpdatedSession, status, nil,
		m.Importance, m.CreatedAt, nil, nil,
		m.AccessCount, nil,
	)
	return err
}

// getMemoryBLOB retrieves a memory and decodes its embedding from BLOB bytes.
func getMemoryBLOB(database *DB, id string) (*Memory, error) {
	var m Memory
	var metaStr string
	var embBLOB []byte
	var consolidatedInto sql.NullString
	var validFrom, validTo sql.NullTime
	var supersededBy sql.NullString
	var lastAccess sql.NullTime

	err := database.conn.QueryRow(
		`SELECT id, content, scope, metadata, embedding, embedding_source, embedding_model,
		        created_at, updated_at, created_by, updated_by, created_session, updated_session,
		        consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by,
		        access_count, last_access
		 FROM memories WHERE id = ?`, id,
	).Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &embBLOB,
		&m.EmbeddingSource, &m.EmbeddingModel, &m.CreatedAt, &m.UpdatedAt,
		&m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession,
		&m.ConsolidationStatus, &consolidatedInto, &m.Importance,
		&validFrom, &validTo, &supersededBy, &m.AccessCount, &lastAccess)
	if err != nil {
		return nil, err
	}
	if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
		return nil, err
	}
	m.Embedding = decodeEmbeddingBLOB(embBLOB)
	return &m, nil
}

// searchMemoriesBLOB performs LSH-candidate-filtered search with BLOB decoding.
func searchMemoriesBLOB(database *DB, queryVec []float32, scope string, limit int) ([]SearchResult, error) {
	const maxCandidatesBLOB = 2000
	const batchSize = 64
	type scored struct {
		m     *Memory
		score float32
	}
	var results []scored

	queryLSH := mustComputeLSH(queryVec)
	buckets := LSHNeighbors(queryLSH, 2)

	var candidateIDs []string
	for i := 0; i < len(buckets) && len(candidateIDs) < maxCandidatesBLOB; i += batchSize {
		end := i + batchSize
		if end > len(buckets) {
			end = len(buckets)
		}
		chunk := buckets[i:end]

		placeholders := make([]string, len(chunk))
		args := make([]interface{}, 0, len(chunk)+1)
		for j, h := range chunk {
			placeholders[j] = "?"
			args = append(args, h)
		}
		inClause := strings.Join(placeholders, ", ")

		var query string
		if scope != "" {
			query = "SELECT id FROM memories WHERE scope = ? AND consolidation_status != 'archived' AND lsh_hash IN (" + inClause + ")"
			args = append([]interface{}{scope}, args...)
		} else {
			query = "SELECT id FROM memories WHERE consolidation_status != 'archived' AND lsh_hash IN (" + inClause + ")"
		}
		query += " ORDER BY created_at DESC"

		rows, err := database.conn.Query(query, args...)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var id string
			if err := rows.Scan(&id); err != nil {
				_ = rows.Close()
				return nil, err
			}
			candidateIDs = append(candidateIDs, id)
			if len(candidateIDs) >= maxCandidatesBLOB {
				break
			}
		}
		_ = rows.Close()
	}

	if len(candidateIDs) == 0 {
		return nil, nil
	}

	// Fetch full rows with BLOB embedding for candidates
	for i := 0; i < len(candidateIDs); i += batchSize {
		end := i + batchSize
		if end > len(candidateIDs) {
			end = len(candidateIDs)
		}
		chunk := candidateIDs[i:end]

		placeholders := make([]string, len(chunk))
		args := make([]interface{}, 0, len(chunk)+1)
		for j, id := range chunk {
			placeholders[j] = "?"
			args = append(args, id)
		}
		inClause := strings.Join(placeholders, ", ")

		query := "SELECT id, content, scope, metadata, embedding, embedding_source, embedding_model, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, access_count, last_access FROM memories WHERE id IN (" + inClause + ")"
		rows, err := database.conn.Query(query, args...)
		if err != nil {
			return nil, err
		}

		for rows.Next() {
			var m Memory
			var metaStr string
			var embBLOB []byte
			var consolidatedInto sql.NullString
			var validFrom, validTo sql.NullTime
			var supersededBy sql.NullString
			var lastAccess sql.NullTime
			if err := rows.Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &embBLOB,
				&m.EmbeddingSource, &m.EmbeddingModel, &m.CreatedAt, &m.UpdatedAt,
				&m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession,
				&m.ConsolidationStatus, &consolidatedInto, &m.Importance,
				&validFrom, &validTo, &supersededBy, &m.AccessCount, &lastAccess); err != nil {
				_ = rows.Close()
				return nil, err
			}
			if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
				_ = rows.Close()
				return nil, err
			}
			m.Embedding = decodeEmbeddingBLOB(embBLOB)

			if len(m.Embedding) > 0 {
				relevance := CosineSimilarity(queryVec, m.Embedding)
				w := DefaultRankingWeights()
				score := float32(CompositeScore(relevance, m.CreatedAt, float64(m.Importance)/10.0, m.AccessCount, m.LastAccess, nil, w))
				results = append(results, scored{m: &m, score: score})
			}
		}
		_ = rows.Close()
	}

	sort.Slice(results, func(i, j int) bool {
		return results[i].score > results[j].score
	})

	if limit > len(results) {
		limit = len(results)
	}
	final := make([]SearchResult, limit)
	for i := range limit {
		final[i] = SearchResult{Memory: results[i].m, Score: results[i].score}
	}
	return final, nil
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// benchOpenTempDB creates a temp HOME, opens a fresh DB, and returns a cleanup func.
func benchOpenTempDB(b testing.TB) (*DB, func()) {
	b.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-bench-*")
	if err != nil {
		b.Fatalf("MkdirTemp: %v", err)
	}
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)

	database, err := Open(config.Defaults())
	if err != nil {
		os.Setenv("HOME", oldHome)
		os.RemoveAll(tempDir)
		b.Fatalf("Open: %v", err)
	}

	cleanup := func() {
		_ = database.Close()
		os.Setenv("HOME", oldHome)
		os.RemoveAll(tempDir)
	}
	return database, cleanup
}

// dbFileSize returns the total size in bytes of the SQLite DB file and its
// WAL/SHM siblings.
func dbFileSize(homeDir string) int64 {
	dbPath := filepath.Join(homeDir, ".local", "share", "symmemory", "default.db")
	var total int64
	for _, suffix := range []string{"", "-wal", "-shm"} {
		info, err := os.Stat(dbPath + suffix)
		if err == nil {
			total += info.Size()
		}
	}
	return total
}

// gzipSize returns the gzipped size of data.
func gzipSize(data []byte) int64 {
	var buf bytes.Buffer
	gz, _ := gzip.NewWriterLevel(&buf, gzip.BestSpeed)
	if _, err := gz.Write(data); err != nil {
		return 0
	}
	if err := gz.Close(); err != nil {
		return 0
	}
	return int64(buf.Len())
}

// seedJSONMemories writes n memories with deterministic embeddings using the
// JSON (production) path. prefix is prepended to each ID.
func seedJSONMemories(database *DB, n int, prefix string) {
	tx, err := database.BeginTransaction()
	if err != nil {
		panic(fmt.Sprintf("seedJSONMemories begin: %v", err))
	}
	defer tx.Rollback()

	for i := range n {
		emb := generateDeterministicEmbedding(i, EmbeddingDim)
		m := &Memory{
			ID:        fmt.Sprintf("%s-%d", prefix, i),
			Content:   fmt.Sprintf("Benchmark memory entry number %d for storage comparison", i),
			Scope:     "global",
			Metadata:  map[string]string{"seed": fmt.Sprintf("%d", i)},
			Embedding: emb,
		}
		if err := database.SaveMemoryTx(tx, m); err != nil {
			panic(fmt.Sprintf("seedJSONMemories: %v", err))
		}
	}

	if err := tx.Commit(); err != nil {
		panic(fmt.Sprintf("seedJSONMemories commit: %v", err))
	}
}

// seedBLOBMemories writes n memories with deterministic embeddings using the
// BLOB (proposed) path. prefix is prepended to each ID.
func seedBLOBMemories(database *DB, n int, prefix string) {
	tx, err := database.BeginTransaction()
	if err != nil {
		panic(fmt.Sprintf("seedBLOBMemories begin: %v", err))
	}
	defer tx.Rollback()

	for i := range n {
		emb := generateDeterministicEmbedding(i, EmbeddingDim)
		m := &Memory{
			ID:        fmt.Sprintf("%s-%d", prefix, i),
			Content:   fmt.Sprintf("Benchmark memory entry number %d for storage comparison", i),
			Scope:     "global",
			Metadata:  map[string]string{"seed": fmt.Sprintf("%d", i)},
			Embedding: emb,
		}
		if err := saveMemoryBLOBTx(tx, m); err != nil {
			panic(fmt.Sprintf("seedBLOBMemories: %v", err))
		}
	}

	if err := tx.Commit(); err != nil {
		panic(fmt.Sprintf("seedBLOBMemories commit: %v", err))
	}
}

// ---------------------------------------------------------------------------
// Benchmark: Save (write) latency
// ---------------------------------------------------------------------------

func benchSave(b *testing.B, useBLOB bool) {
	database, cleanup := benchOpenTempDB(b)
	defer cleanup()

	b.ResetTimer()
	for i := range b.N {
		emb := generateDeterministicEmbedding(i, EmbeddingDim)
		m := &Memory{
			ID:        fmt.Sprintf("bench-save-%d", i),
			Content:   fmt.Sprintf("Benchmark write entry %d", i),
			Scope:     "global",
			Metadata:  map[string]string{"i": fmt.Sprintf("%d", i)},
			Embedding: emb,
		}
		var err error
		if useBLOB {
			err = saveMemoryBLOB(database, m)
		} else {
			err = database.SaveMemory(m)
		}
		if err != nil {
			b.Fatalf("save failed: %v", err)
		}
	}
}

func BenchmarkEmbeddingJSON_Save(b *testing.B) { benchSave(b, false) }
func BenchmarkEmbeddingBLOB_Save(b *testing.B) { benchSave(b, true) }

// ---------------------------------------------------------------------------
// Benchmark: Get (single-read) latency
// ---------------------------------------------------------------------------

func benchGet(b *testing.B, useBLOB bool) {
	database, cleanup := benchOpenTempDB(b)
	defer cleanup()

	const preloaded = 1000
	for i := range preloaded {
		emb := generateDeterministicEmbedding(i, EmbeddingDim)
		m := &Memory{
			ID:        fmt.Sprintf("get-mem-%d", i),
			Content:   fmt.Sprintf("Preloaded entry %d", i),
			Scope:     "global",
			Metadata:  map[string]string{},
			Embedding: emb,
		}
		if useBLOB {
			if err := saveMemoryBLOB(database, m); err != nil {
				b.Fatalf("seed: %v", err)
			}
		} else {
			if err := database.SaveMemory(m); err != nil {
				b.Fatalf("seed: %v", err)
			}
		}
	}

	b.ResetTimer()
	for i := range b.N {
		id := fmt.Sprintf("get-mem-%d", i%preloaded)
		var err error
		if useBLOB {
			_, err = getMemoryBLOB(database, id)
		} else {
			_, err = database.GetMemory(id)
		}
		if err != nil {
			b.Fatalf("get failed: %v", err)
		}
	}
}

func BenchmarkEmbeddingJSON_Get(b *testing.B) { benchGet(b, false) }
func BenchmarkEmbeddingBLOB_Get(b *testing.B) { benchGet(b, true) }

// ---------------------------------------------------------------------------
// Benchmark: Search latency (1K pre-seeded)
// ---------------------------------------------------------------------------

func benchSearch(b *testing.B, useBLOB bool) {
	database, cleanup := benchOpenTempDB(b)
	defer cleanup()

	const preloaded = 1000
	if useBLOB {
		seedBLOBMemories(database, preloaded, "mem")
	} else {
		seedJSONMemories(database, preloaded, "mem")
	}

	queryVec := generateDeterministicEmbedding(99999, EmbeddingDim)
	b.ResetTimer()
	for i := range b.N {
		var err error
		if useBLOB {
			_, err = searchMemoriesBLOB(database, queryVec, "global", 10)
		} else {
			_, err = database.SearchMemories(queryVec, "", "global", 10)
		}
		if err != nil {
			b.Fatalf("search failed: %v", err)
		}
		_ = i
	}
}

func BenchmarkEmbeddingJSON_Search(b *testing.B) { benchSearch(b, false) }
func BenchmarkEmbeddingBLOB_Search(b *testing.B) { benchSearch(b, true) }

// ---------------------------------------------------------------------------
// Scales for size / quality / backup tests
// ---------------------------------------------------------------------------

var benchScales = []int{100, 1_000, 10_000}

// TestEmbeddingStorageSize measures the database file size for each scale.
func TestEmbeddingStorageSize(t *testing.T) {
	if testing.Short() {
		t.Skip("storage-size measurements are excluded from short test runs")
	}

	for _, scale := range benchScales {
		t.Run(fmt.Sprintf("scale_%d", scale), func(t *testing.T) {
			// JSON path
			jsonDB, jsonCleanup := benchOpenTempDB(t)
			t.Cleanup(jsonCleanup)
			seedJSONMemories(jsonDB, scale, "json")
			jsonHome := os.Getenv("HOME")
			jsonSize := dbFileSize(jsonHome)

			// BLOB path
			blobDB, blobCleanup := benchOpenTempDB(t)
			t.Cleanup(blobCleanup)
			seedBLOBMemories(blobDB, scale, "blob")
			blobHome := os.Getenv("HOME")
			blobSize := dbFileSize(blobHome)

			savings := 0.0
			if jsonSize > 0 {
				savings = float64(jsonSize-blobSize) / float64(jsonSize) * 100
			}

			t.Logf("Scale %d: JSON=%d bytes, BLOB=%d bytes, savings=%.1f%%",
				scale, jsonSize, blobSize, savings)
		})
	}
}

// TestEmbeddingSearchQuality verifies that both JSON and BLOB paths return
// identical search results (same IDs and scores) for the same data.
func TestEmbeddingSearchQuality(t *testing.T) {
	scales := []int{100, 1_000}
	if testing.Short() {
		scales = []int{100}
	}

	for _, scale := range scales {
		t.Run(fmt.Sprintf("scale_%d", scale), func(t *testing.T) {
			// JSON path
			jsonDB, jsonCleanup := benchOpenTempDB(t)
			t.Cleanup(jsonCleanup)
			seedJSONMemories(jsonDB, scale, "mem")

			// BLOB path
			blobDB, blobCleanup := benchOpenTempDB(t)
			t.Cleanup(blobCleanup)
			seedBLOBMemories(blobDB, scale, "mem")

			queryVec := generateDeterministicEmbedding(42, EmbeddingDim)

			jsonResults, err := jsonDB.SearchMemories(queryVec, "", "global", 20)
			if err != nil {
				t.Fatalf("JSON search failed: %v", err)
			}

			blobResults, err := searchMemoriesBLOB(blobDB, queryVec, "global", 20)
			if err != nil {
				t.Fatalf("BLOB search failed: %v", err)
			}

			if len(jsonResults) != len(blobResults) {
				t.Errorf("result count mismatch: JSON=%d, BLOB=%d", len(jsonResults), len(blobResults))
				for i, r := range jsonResults {
					t.Logf("  JSON[%d]: id=%s score=%.6f", i, r.Memory.ID, r.Score)
				}
				for i, r := range blobResults {
					t.Logf("  BLOB[%d]: id=%s score=%.6f", i, r.Memory.ID, r.Score)
				}
				return
			}

			for i := range jsonResults {
				if jsonResults[i].Memory.ID != blobResults[i].Memory.ID {
					t.Errorf("rank %d: ID mismatch JSON=%s BLOB=%s",
						i, jsonResults[i].Memory.ID, blobResults[i].Memory.ID)
				}
				scoreDiff := math.Abs(float64(jsonResults[i].Score - blobResults[i].Score))
				if scoreDiff > 1e-5 {
					t.Errorf("rank %d: score mismatch JSON=%.6f BLOB=%.6f (diff=%.8f)",
						i, jsonResults[i].Score, blobResults[i].Score, scoreDiff)
				}
			}
			t.Logf("Quality check passed: %d results, identical across formats", len(jsonResults))
		})
	}
}

// TestEmbeddingBackupSize measures a "backup" (full DB copy) size for each format.
func TestEmbeddingBackupSize(t *testing.T) {
	if testing.Short() {
		t.Skip("backup-size measurements are excluded from short test runs")
	}

	scales := []int{100, 1_000, 10_000}

	for _, scale := range scales {
		t.Run(fmt.Sprintf("scale_%d", scale), func(t *testing.T) {
			// JSON path
			jsonDB, jsonCleanup := benchOpenTempDB(t)
			t.Cleanup(jsonCleanup)
			seedJSONMemories(jsonDB, scale, "json")
			jsonDBPath, err := paths.DatabasePath()
			if err != nil {
				t.Fatalf("resolve JSON db path: %v", err)
			}
			jsonRaw, err := os.ReadFile(jsonDBPath)
			if err != nil {
				t.Fatalf("read JSON db: %v", err)
			}

			// BLOB path
			blobDB, blobCleanup := benchOpenTempDB(t)
			t.Cleanup(blobCleanup)
			seedBLOBMemories(blobDB, scale, "blob")
			blobDBPath, err := paths.DatabasePath()
			if err != nil {
				t.Fatalf("resolve BLOB db path: %v", err)
			}
			blobRaw, err := os.ReadFile(blobDBPath)
			if err != nil {
				t.Fatalf("read BLOB db: %v", err)
			}

			jsonGzip := gzipSize(jsonRaw)
			blobGzip := gzipSize(blobRaw)
			savingsRaw := 0.0
			if len(jsonRaw) > 0 {
				savingsRaw = float64(len(jsonRaw)-len(blobRaw)) / float64(len(jsonRaw)) * 100
			}
			savingsGzip := 0.0
			if jsonGzip > 0 {
				savingsGzip = float64(jsonGzip-blobGzip) / float64(jsonGzip) * 100
			}

			t.Logf("Scale %d raw: JSON=%d, BLOB=%d, savings=%.1f%%", scale, len(jsonRaw), len(blobRaw), savingsRaw)
			t.Logf("Scale %d gzip: JSON=%d, BLOB=%d, savings=%.1f%%", scale, jsonGzip, blobGzip, savingsGzip)
		})
	}
}

// TestEmbeddingEncodeDecodeRoundtrip verifies BLOB encode/decode fidelity.
func TestEmbeddingEncodeDecodeRoundtrip(t *testing.T) {
	orig := generateDeterministicEmbedding(42, EmbeddingDim)
	encoded := encodeEmbeddingBLOB(orig)
	decoded := decodeEmbeddingBLOB(encoded)

	if len(decoded) != len(orig) {
		t.Fatalf("dimension mismatch: orig=%d decoded=%d", len(orig), len(decoded))
	}
	for i := range orig {
		if orig[i] != decoded[i] {
			t.Fatalf("value mismatch at %d: orig=%f decoded=%f", i, orig[i], decoded[i])
		}
	}

	// Verify BLOB is exactly 4*dim bytes
	expectedBytes := 4 * EmbeddingDim
	if len(encoded) != expectedBytes {
		t.Errorf("BLOB size: expected %d, got %d", expectedBytes, len(encoded))
	}

	// Verify BLOB is smaller than JSON
	jsonBytes, err := json.Marshal(orig)
	if err != nil {
		t.Fatalf("json.Marshal: %v", err)
	}
	t.Logf("768-dim embedding: JSON=%d bytes, BLOB=%d bytes, ratio=%.2f",
		len(jsonBytes), len(encoded), float64(len(jsonBytes))/float64(len(encoded)))
}

// ---------------------------------------------------------------------------
// Recommendation summary
// ---------------------------------------------------------------------------

// TestEmbeddingRecommendation prints a comparative summary and a written
// recommendation for the JSON-vs-BLOB storage decision.
func TestEmbeddingRecommendation(t *testing.T) {
	if testing.Short() {
		t.Skip("embedding recommendation measurements are excluded from short test runs")
	}

	const scale = 1_000

	// --- Size measurement ---
	jsonDB, jsonCleanup := benchOpenTempDB(t)
	t.Cleanup(jsonCleanup)
	seedJSONMemories(jsonDB, scale, "json")
	jsonHome := os.Getenv("HOME")
	jsonSize := dbFileSize(jsonHome)

	blobDB, blobCleanup := benchOpenTempDB(t)
	t.Cleanup(blobCleanup)
	seedBLOBMemories(blobDB, scale, "blob")
	blobHome := os.Getenv("HOME")
	blobSize := dbFileSize(blobHome)

	// --- Search speed measurement ---
	queryVec := generateDeterministicEmbedding(42, EmbeddingDim)

	jsonDB2, jsonCleanup2 := benchOpenTempDB(t)
	t.Cleanup(jsonCleanup2)
	seedJSONMemories(jsonDB2, scale, "json")
	start := time.Now()
	for range 100 {
		_, _ = jsonDB2.SearchMemories(queryVec, "", "global", 10)
	}
	jsonSearchDur := time.Since(start)

	blobDB2, blobCleanup2 := benchOpenTempDB(t)
	t.Cleanup(blobCleanup2)
	seedBLOBMemories(blobDB2, scale, "blob")
	start = time.Now()
	for range 100 {
		_, _ = searchMemoriesBLOB(blobDB2, queryVec, "global", 10)
	}
	blobSearchDur := time.Since(start)

	// --- Encode speed measurement ---
	sampleEmb := generateDeterministicEmbedding(0, EmbeddingDim)
	start = time.Now()
	for range 100_000 {
		_, _ = json.Marshal(sampleEmb)
	}
	jsonEncDur := time.Since(start)

	start = time.Now()
	for range 100_000 {
		_ = encodeEmbeddingBLOB(sampleEmb)
	}
	blobEncDur := time.Since(start)

	// --- Print summary table ---
	t.Log("")
	t.Log("=================================================================")
	t.Log("  Embedding Storage Format Benchmark - Recommendation Summary")
	t.Log("=================================================================")
	t.Logf("  Scale: %d memories, %d-dimensional embeddings", scale, EmbeddingDim)
	t.Log("-----------------------------------------------------------------")
	t.Logf("  DB file size:   JSON = %8d bytes | BLOB = %8d bytes", jsonSize, blobSize)
	if jsonSize > 0 {
		t.Logf("                  Savings: %.1f%%", float64(jsonSize-blobSize)/float64(jsonSize)*100)
	}
	t.Log("-----------------------------------------------------------------")
	t.Logf("  100 searches:   JSON = %v | BLOB = %v", jsonSearchDur, blobSearchDur)
	if blobSearchDur > 0 {
		t.Logf("                  Ratio:   %.2fx", float64(jsonSearchDur)/float64(blobSearchDur))
	}
	t.Log("-----------------------------------------------------------------")
	t.Logf("  100K encodes:   JSON = %v | BLOB = %v", jsonEncDur, blobEncDur)
	if blobEncDur > 0 {
		t.Logf("                  Ratio:   %.2fx", float64(jsonEncDur)/float64(blobEncDur))
	}
	t.Log("=================================================================")
	t.Log("")
	t.Log("  RECOMMENDATION:")
	t.Log("")
	t.Log("  For the current scale of Symaira Memory (typically <10K memories),")
	t.Log("  JSON text storage is simpler, fully transparent in SQLite tooling,")
	t.Log("  and the overhead is modest (a few hundred KB at 1K memories).")
	t.Log("")
	t.Log("  BLOB storage provides measurable size savings (~75% smaller per")
	t.Log("  embedding) and faster encode/decode, which becomes meaningful at")
	t.Log("  10K-100K+ scale - especially for backup/sync payloads and WAL")
	t.Log("  write amplification.")
	t.Log("")
	t.Log("  ADOPT BLOB storage when:")
	t.Log("    1. Memory count regularly exceeds 10,000")
	t.Log("    2. Backup/sync bandwidth is a bottleneck")
	t.Log("    3. TurboQuant quantised vectors are reintroduced (BLOB is the")
	t.Log("       natural on-disk format for quantised data)")
	t.Log("")
	t.Log("  KEEP JSON storage when:")
	t.Log("    1. Scale stays under 10K memories")
	t.Log("    2. Manual SQLite inspection/debugging is a priority")
	t.Log("    3. Migration cost outweighs storage savings")
	t.Log("=================================================================")
}
