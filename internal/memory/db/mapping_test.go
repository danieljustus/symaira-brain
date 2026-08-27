package db

import (
	"strings"
	"testing"
	"time"
)

// TestMemoryColumnMapping locks the row-mapping contract so that future
// schema growth cannot silently desync the full scan, lite scan, and
// insert/upsert parameter lists.
func TestMemoryColumnMapping(t *testing.T) {
	// 1. Canonical list is 33 columns — the single source of truth.
	if got, want := len(memoryCanonicalColumns), 33; got != want {
		t.Fatalf("memoryCanonicalColumns length: got %d, want %d", got, want)
	}

	// 2. Full projection is 30 columns.
	fullCols := strings.Split(memoryColumns, ", ")
	if got, want := len(fullCols), 30; got != want {
		t.Fatalf("memoryColumns length: got %d, want %d", got, want)
	}

	// 3. Lite projection is 25 columns.
	liteCols := strings.Split(memoryColumnsLite, ", ")
	if got, want := len(liteCols), 25; got != want {
		t.Fatalf("memoryColumnsLite length: got %d, want %d", got, want)
	}

	// 4. Lite is exactly full minus the 5 embedding-related columns.
	liteSet := make(map[string]bool, len(liteCols))
	for _, c := range liteCols {
		liteSet[c] = true
	}
	omitted := 0
	for _, c := range fullCols {
		if !liteSet[c] {
			if _, ok := memoryLiteOmitted[c]; !ok {
				t.Errorf("full column %q missing from lite but not in memoryLiteOmitted", c)
			}
			omitted++
		}
	}
	if omitted != 5 {
		t.Errorf("omitted columns: got %d, want 5", omitted)
	}

	// 5. Full projection omits exactly the 3 columns that can be recomputed
	// (embedding_dim, content_hash, lsh_hash) and are only needed on write.
	fullSet := make(map[string]bool, len(fullCols))
	for _, c := range fullCols {
		if fullSet[c] {
			t.Errorf("duplicate column in memoryColumns: %q", c)
		}
		fullSet[c] = true
	}
	for _, c := range memoryCanonicalColumns {
		if !fullSet[c] && c != "embedding_dim" && c != "content_hash" && c != "lsh_hash" {
			t.Errorf("canonical column %q missing from memoryColumns", c)
		}
	}
	// Verify the 3 omitted columns are the expected ones.
	for _, c := range []string{"embedding_dim", "content_hash", "lsh_hash"} {
		if fullSet[c] {
			t.Errorf("computed column %q should not be in memoryColumns", c)
		}
	}

	// 6. Insert args count matches the 33 canonical columns 1:1.
	m := &Memory{
		Content: "test-mapping",
		Scope:   "global",
	}
	args, err := MemoryInsertArgs(m, mustTime("2025-01-01T00:00:00Z"), false)
	if err != nil {
		t.Fatalf("MemoryInsertArgs error: %v", err)
	}
	if got, want := len(args), len(memoryCanonicalColumns); got != want {
		t.Errorf("MemoryInsertArgs length: got %d, want %d", got, want)
	}
}

func mustTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		panic(err)
	}
	return t
}
