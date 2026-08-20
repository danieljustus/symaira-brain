package db

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestLogQueryReturnsIDAndRecordsResults(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	// Result rows reference real memories (FK), so save them first.
	saveResultTestMemories(t, database)

	queryID, err := database.LogQuery("actor-1", "global", "sess-1", "memory_search", "how to deploy", `{"query":"how to deploy"}`, 42)
	if err != nil {
		t.Fatalf("LogQuery failed: %v", err)
	}
	if queryID == "" {
		t.Fatal("LogQuery returned an empty query id")
	}

	refs := []QueryResultRef{
		{MemoryID: "mem-1", Rank: 0, Score: 0.91},
		{MemoryID: "mem-2", Rank: 1, Score: 0.82},
		{MemoryID: "mem-3", Rank: 2, Score: 0.73},
	}
	if err := database.RecordQueryResults(queryID, refs); err != nil {
		t.Fatalf("RecordQueryResults failed: %v", err)
	}

	got, err := database.GetQueryLogResults(queryID)
	if err != nil {
		t.Fatalf("GetQueryLogResults failed: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("expected 3 result rows, got %d", len(got))
	}
	for i, r := range got {
		if r.Rank != i {
			t.Errorf("row %d: expected rank %d, got %d", i, i, r.Rank)
		}
		if r.MemoryID != refs[i].MemoryID {
			t.Errorf("row %d: expected memory %s, got %s", i, refs[i].MemoryID, r.MemoryID)
		}
		if r.Score != refs[i].Score {
			t.Errorf("row %d: expected score %.2f, got %.2f", i, refs[i].Score, r.Score)
		}
		if r.QueryID != queryID {
			t.Errorf("row %d: query_id mismatch: %s", i, r.QueryID)
		}
	}
}

// saveResultTestMemories creates the memories referenced by result rows in
// these tests (query_log_results has a foreign key to memories.id).
func saveResultTestMemories(t *testing.T, database *DB) {
	t.Helper()
	for _, id := range []string{"mem-1", "mem-2", "mem-3", "mem-result-1"} {
		if err := database.SaveMemory(&Memory{
			ID:       id,
			Content:  "Memory " + id,
			Scope:    "global",
			Metadata: map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory(%s) failed: %v", id, err)
		}
	}
}

func TestRecordQueryResultsEmptyAndUnknown(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	// Empty result list is a no-op.
	queryID, err := database.LogQuery("actor-1", "global", "", "memory_search", "nothing", "", 1)
	if err != nil {
		t.Fatal(err)
	}
	if err := database.RecordQueryResults(queryID, nil); err != nil {
		t.Fatalf("recording empty results failed: %v", err)
	}
	got, err := database.GetQueryLogResults(queryID)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 0 {
		t.Errorf("expected no rows for empty result set, got %d", len(got))
	}

	// Unknown query id resolves to an empty list, not an error.
	got, err = database.GetQueryLogResults("does-not-exist")
	if err != nil {
		t.Fatalf("unknown query id errored: %v", err)
	}
	if len(got) != 0 {
		t.Errorf("expected no rows for unknown query, got %d", len(got))
	}
}

func TestRecordQueryResultsDisabled(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	cfg.QueryLog.RecordResults = false
	database := openTestDBWithConfig(t, cfg)

	queryID, err := database.LogQuery("actor-1", "global", "", "memory_search", "q", "", 1)
	if err != nil {
		t.Fatalf("LogQuery failed: %v", err)
	}
	if err := database.RecordQueryResults(queryID, []QueryResultRef{{MemoryID: "mem-1", Rank: 0, Score: 0.5}}); err != nil {
		t.Fatalf("RecordQueryResults failed: %v", err)
	}
	got, err := database.GetQueryLogResults(queryID)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 0 {
		t.Errorf("recording disabled: expected no result rows, got %d", len(got))
	}
}

func TestQueryLogResultsCascadeOnMemoryDelete(t *testing.T) {
	database := openTestDB(t)
	defer func() { _ = database.Close() }()

	if err := database.SaveMemory(&Memory{
		ID:       "mem-cascade",
		Content:  "Something to retrieve.",
		Scope:    "global",
		Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	queryID, err := database.LogQuery("actor-1", "global", "", "memory_search", "retrieve", "", 5)
	if err != nil {
		t.Fatal(err)
	}
	if err := database.RecordQueryResults(queryID, []QueryResultRef{{MemoryID: "mem-cascade", Rank: 0, Score: 0.9}}); err != nil {
		t.Fatal(err)
	}

	// Deleting the memory must cascade away its result rows.
	if err := database.DeleteMemory("mem-cascade"); err != nil {
		t.Fatalf("DeleteMemory failed: %v", err)
	}

	got, err := database.GetQueryLogResults(queryID)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 0 {
		t.Errorf("memory delete must cascade result rows, got %d dangling rows", len(got))
	}
}

func TestQueryLogPruneRemovesResultRows(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	cfg.QueryLog.MaxEntries = 2 // prune after every third insert
	database := openTestDBWithConfig(t, cfg)
	saveResultTestMemories(t, database)

	var ids []string
	for i := 0; i < 3; i++ {
		qid, err := database.LogQuery("actor-1", "global", "", "memory_search", "q", "", 1)
		if err != nil {
			t.Fatalf("LogQuery %d failed: %v", i, err)
		}
		ids = append(ids, qid)
		if err := database.RecordQueryResults(qid, []QueryResultRef{{MemoryID: "mem-result-1", Rank: 0, Score: 0.5}}); err != nil {
			t.Fatal(err)
		}
	}

	// The oldest query was pruned by the cap; its result rows must be gone.
	oldest, err := database.GetQueryLogResults(ids[0])
	if err != nil {
		t.Fatal(err)
	}
	if len(oldest) != 0 {
		t.Errorf("pruned query must not keep result rows, got %d", len(oldest))
	}
	// The two newest queries survive with their rows.
	for _, id := range ids[1:] {
		got, err := database.GetQueryLogResults(id)
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 {
			t.Errorf("surviving query %s lost its result row", id)
		}
	}
}
