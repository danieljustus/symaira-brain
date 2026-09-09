//go:build memory_large

package db

import (
	"bytes"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"testing"
	"time"
)

const (
	defaultMemoryStorageScale = 1_000
	maxMemoryStorageScale     = 10_000
)

// configuredMemoryStorageScale bounds explicit storage measurements so a
// mistaken environment value cannot recreate an unbounded disk-filling test.
func configuredMemoryStorageScale(t testing.TB) int {
	t.Helper()
	scale := defaultMemoryStorageScale
	if raw := os.Getenv("SYMBRAIN_MEMORY_STORAGE_SCALE"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed <= 0 {
			t.Fatalf("SYMBRAIN_MEMORY_STORAGE_SCALE must be a positive integer, got %q", raw)
		}
		scale = parsed
	}
	if scale > maxMemoryStorageScale {
		t.Fatalf("SYMBRAIN_MEMORY_STORAGE_SCALE=%d exceeds safety limit %d", scale, maxMemoryStorageScale)
	}
	return scale
}

func memoryStorageScales(t testing.TB) []int {
	maxScale := configuredMemoryStorageScale(t)
	if maxScale <= 100 {
		return []int{maxScale}
	}
	if maxScale <= defaultMemoryStorageScale {
		return []int{100, maxScale}
	}
	return []int{100, defaultMemoryStorageScale, maxScale}
}

// gzipSize returns the gzipped size of data.
func gzipSize(data []byte) (int64, error) {
	var buf bytes.Buffer
	gz, err := gzip.NewWriterLevel(&buf, gzip.BestSpeed)
	if err != nil {
		return 0, err
	}
	if _, err := gz.Write(data); err != nil {
		return 0, err
	}
	if err := gz.Close(); err != nil {
		return 0, err
	}
	return int64(buf.Len()), nil
}

// stableDatabaseSnapshot checkpoints WAL contents into the main database,
// closes the connection, and reads the resulting SQLite backup image. This
// matches a real file backup: no uncheckpointed WAL pages are omitted.
func stableDatabaseSnapshot(t testing.TB, database *DB) []byte {
	t.Helper()
	databasePath := database.Path()
	if databasePath == "" {
		t.Fatal("database has no resolved path")
	}

	var busy, walFrames, checkpointedFrames int
	if err := database.Conn().QueryRow("PRAGMA wal_checkpoint(TRUNCATE)").Scan(&busy, &walFrames, &checkpointedFrames); err != nil {
		t.Fatalf("checkpoint database %q: %v", databasePath, err)
	}
	if busy != 0 {
		t.Fatalf("checkpoint database %q remained busy: busy=%d wal_frames=%d checkpointed=%d", databasePath, busy, walFrames, checkpointedFrames)
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close database %q: %v", databasePath, err)
	}
	raw, err := os.ReadFile(databasePath)
	if err != nil {
		t.Fatalf("read checkpointed database %q: %v", databasePath, err)
	}
	return raw
}

func assertRawMeasurement(t testing.TB, format string, scale int, raw []byte) {
	t.Helper()
	size := int64(len(raw))
	minimum := int64(4 * EmbeddingDim * scale)
	if size <= 4096 || size < minimum {
		t.Fatalf("%s measurement is implausible: %d bytes for %d memories (minimum %d)", format, size, scale, minimum)
	}
}

func assertGzipMeasurement(t testing.TB, format string, compressedSize int64) {
	t.Helper()
	if compressedSize <= 0 {
		t.Fatalf("%s gzip measurement is empty: %d bytes", format, compressedSize)
	}
}

// TestEmbeddingStorageSize measures the checkpointed database size for each scale.
func TestEmbeddingStorageSize(t *testing.T) {
	if testing.Short() {
		t.Skip("storage-size measurements are excluded from short test runs")
	}

	for _, scale := range memoryStorageScales(t) {
		t.Run(fmt.Sprintf("scale_%d", scale), func(t *testing.T) {
			jsonDB := benchOpenTempDB(t)
			seedJSONMemories(jsonDB, scale, "json")
			jsonRaw := stableDatabaseSnapshot(t, jsonDB)
			assertRawMeasurement(t, "JSON storage", scale, jsonRaw)

			blobDB := benchOpenTempDB(t)
			seedBLOBMemories(blobDB, scale, "blob")
			blobRaw := stableDatabaseSnapshot(t, blobDB)
			assertRawMeasurement(t, "BLOB storage", scale, blobRaw)

			jsonSize := int64(len(jsonRaw))
			blobSize := int64(len(blobRaw))
			savings := float64(jsonSize-blobSize) / float64(jsonSize) * 100
			t.Logf("Scale %d: JSON=%d bytes, BLOB=%d bytes, savings=%.1f%%", scale, jsonSize, blobSize, savings)
		})
	}
}

// TestEmbeddingBackupSize measures a checkpointed, full DB copy size for each format.
func TestEmbeddingBackupSize(t *testing.T) {
	if testing.Short() {
		t.Skip("backup-size measurements are excluded from short test runs")
	}

	for _, scale := range memoryStorageScales(t) {
		t.Run(fmt.Sprintf("scale_%d", scale), func(t *testing.T) {
			jsonDB := benchOpenTempDB(t)
			seedJSONMemories(jsonDB, scale, "json")
			jsonRaw := stableDatabaseSnapshot(t, jsonDB)
			assertRawMeasurement(t, "JSON backup", scale, jsonRaw)

			blobDB := benchOpenTempDB(t)
			seedBLOBMemories(blobDB, scale, "blob")
			blobRaw := stableDatabaseSnapshot(t, blobDB)
			assertRawMeasurement(t, "BLOB backup", scale, blobRaw)

			jsonGzip, err := gzipSize(jsonRaw)
			if err != nil {
				t.Fatalf("gzip JSON backup: %v", err)
			}
			blobGzip, err := gzipSize(blobRaw)
			if err != nil {
				t.Fatalf("gzip BLOB backup: %v", err)
			}
			assertGzipMeasurement(t, "JSON backup", jsonGzip)
			assertGzipMeasurement(t, "BLOB backup", blobGzip)

			jsonSize := int64(len(jsonRaw))
			blobSize := int64(len(blobRaw))
			savingsRaw := float64(jsonSize-blobSize) / float64(jsonSize) * 100
			savingsGzip := float64(jsonGzip-blobGzip) / float64(jsonGzip) * 100
			t.Logf("Scale %d raw: JSON=%d, BLOB=%d, savings=%.1f%%", scale, jsonSize, blobSize, savingsRaw)
			t.Logf("Scale %d gzip: JSON=%d, BLOB=%d, savings=%.1f%%", scale, jsonGzip, blobGzip, savingsGzip)
		})
	}
}

// Recommendation summary
// ---------------------------------------------------------------------------

// TestEmbeddingRecommendation prints a comparative summary and a written
// recommendation for the JSON-vs-BLOB storage decision.
func TestEmbeddingRecommendation(t *testing.T) {
	if testing.Short() {
		t.Skip("embedding recommendation measurements are excluded from short test runs")
	}

	scale := configuredMemoryStorageScale(t)

	// --- Size measurement ---
	jsonDB := benchOpenTempDB(t)
	seedJSONMemories(jsonDB, scale, "json")
	jsonRaw := stableDatabaseSnapshot(t, jsonDB)
	assertRawMeasurement(t, "JSON recommendation", scale, jsonRaw)
	jsonSize := int64(len(jsonRaw))

	blobDB := benchOpenTempDB(t)
	seedBLOBMemories(blobDB, scale, "blob")
	blobRaw := stableDatabaseSnapshot(t, blobDB)
	assertRawMeasurement(t, "BLOB recommendation", scale, blobRaw)
	blobSize := int64(len(blobRaw))

	// --- Search speed measurement ---
	queryVec := generateDeterministicEmbedding(42, EmbeddingDim)

	jsonDB2 := benchOpenTempDB(t)

	seedJSONMemories(jsonDB2, scale, "json")
	start := time.Now()
	for range 100 {
		_, _ = jsonDB2.SearchMemories(queryVec, "", "global", 10)
	}
	jsonSearchDur := time.Since(start)

	blobDB2 := benchOpenTempDB(t)

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
