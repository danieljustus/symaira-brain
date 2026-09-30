package audit

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// TestTailReadsProductionEnvelopes pins #462: entries written through the
// hash-chained production Logger must round-trip through the tail readers,
// alongside legacy plain JSONL lines.
func TestTailReadsProductionEnvelopes(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	logger, err := Open("chain", Config{Enabled: true})
	if err != nil {
		t.Fatal(err)
	}
	logger.Log("memory", "memory_search", json.RawMessage(`{"query":"term"}`), time.Millisecond, "ok", Exposure{})
	logger.LogDegradation("vault", "unavailable", "warn")
	if err := logger.Close(); err != nil {
		t.Fatal(err)
	}

	entries, err := TailEntries("chain", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 2 || entries[0].Tool != "memory_search" || entries[0].Profile != "chain" || entries[1].Status != "degraded" {
		t.Fatalf("tail entries = %+v", entries)
	}

	degradations, err := LatestDegradations("chain")
	if err != nil {
		t.Fatal(err)
	}
	if len(degradations) != 1 || degradations[0].Server != "vault" || degradations[0].Reason != "unavailable" {
		t.Fatalf("degradations = %+v", degradations)
	}
}

func TestTailStillReadsLegacyPlainLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "legacy.jsonl")
	line := `{"timestamp":"2026-01-01T00:00:00Z","profile":"p","server":"memory","tool":"memory_list","status":"ok"}` + "\n"
	if err := os.WriteFile(path, []byte(line+"not json\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	entries, err := tailEntriesBounded(path, 0, nil, false)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Tool != "memory_list" {
		t.Fatalf("entries = %+v", entries)
	}
}
