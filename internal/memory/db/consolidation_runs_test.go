package db

import (
	"reflect"
	"strings"
	"testing"
	"time"
)

// insertRunRow inserts a consolidation_runs row directly, bypassing
// SaveConsolidationRun, so tests can control the exact stored run_at string.
func insertRunRow(t *testing.T, database *DB, id, runAt, status, summary string, archived, consolidated int) {
	t.Helper()
	if _, err := database.conn.Exec(
		`INSERT INTO consolidation_runs (id, run_at, status, summary_json, total_archived, total_consolidated)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		id, runAt, status, summary, archived, consolidated,
	); err != nil {
		t.Fatalf("failed to insert run row %s: %v", id, err)
	}
}

func TestSaveConsolidationRunInsertsRow(t *testing.T) {
	database := openTestDB(t)
	runAt := time.Date(2026, 1, 15, 10, 30, 0, 0, time.UTC)
	run := &ConsolidationRun{
		ID:                "run-save-1",
		RunAt:             runAt,
		Status:            "completed",
		SummaryJSON:       `{"new_memory_ids":["n1"],"archived_ids":["a1"]}`,
		TotalArchived:     1,
		TotalConsolidated: 1,
	}
	if err := database.SaveConsolidationRun(run); err != nil {
		t.Fatalf("SaveConsolidationRun: %v", err)
	}

	var id, storedRunAt, status, summary string
	var archived, consolidated int
	err := database.conn.QueryRow(
		`SELECT id, run_at, status, summary_json, total_archived, total_consolidated
		 FROM consolidation_runs WHERE id = ?`, run.ID,
	).Scan(&id, &storedRunAt, &status, &summary, &archived, &consolidated)
	if err != nil {
		t.Fatalf("failed to read back run row: %v", err)
	}
	if id != run.ID || status != run.Status || summary != run.SummaryJSON {
		t.Errorf("row mismatch: id=%q status=%q summary=%q", id, status, summary)
	}
	if archived != run.TotalArchived || consolidated != run.TotalConsolidated {
		t.Errorf("totals mismatch: archived=%d consolidated=%d", archived, consolidated)
	}
	if storedRunAt == "" {
		t.Error("expected non-empty stored run_at")
	}
}

func TestGetLastCompletedRunReturnsLatest(t *testing.T) {
	database := openTestDB(t)
	insertRunRow(t, database, "run-old", "2026-01-01 00:00:00", "completed", `{"new_memory_ids":["old"]}`, 1, 1)
	insertRunRow(t, database, "run-new", "2026-01-02 00:00:00", "completed", `{"new_memory_ids":["new"]}`, 2, 2)

	run, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if run == nil {
		t.Fatal("expected a run, got nil")
	}
	if run.ID != "run-new" {
		t.Errorf("expected run-new, got %q", run.ID)
	}
	if run.Status != "completed" || run.SummaryJSON != `{"new_memory_ids":["new"]}` {
		t.Errorf("run fields mismatch: %+v", run)
	}
	if run.TotalArchived != 2 || run.TotalConsolidated != 2 {
		t.Errorf("totals mismatch: %+v", run)
	}
	wantRunAt := time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC)
	if !run.RunAt.Equal(wantRunAt) {
		t.Errorf("RunAt = %v, want %v", run.RunAt, wantRunAt)
	}
}

func TestGetLastCompletedRunParsesTimezoneSuffix(t *testing.T) {
	database := openTestDB(t)
	insertRunRow(t, database, "run-tz", "2026-03-05 08:15:00+00:00", "completed", "{}", 0, 0)

	run, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if run == nil {
		t.Fatal("expected a run, got nil")
	}
	wantRunAt := time.Date(2026, 3, 5, 8, 15, 0, 0, time.UTC)
	if !run.RunAt.Equal(wantRunAt) {
		t.Errorf("RunAt = %v, want %v", run.RunAt, wantRunAt)
	}
}

func TestGetLastCompletedRunSkipsUndone(t *testing.T) {
	database := openTestDB(t)
	insertRunRow(t, database, "run-undone", "2026-02-01 00:00:00", "undone", "{}", 0, 0)
	insertRunRow(t, database, "run-completed", "2026-01-01 00:00:00", "completed", "{}", 0, 0)

	run, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if run == nil {
		t.Fatal("expected the completed run, got nil")
	}
	if run.ID != "run-completed" {
		t.Errorf("expected run-completed (undone newer run must be skipped), got %q", run.ID)
	}
}

func TestGetLastCompletedRunEmpty(t *testing.T) {
	database := openTestDB(t)
	run, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if run != nil {
		t.Errorf("expected nil run, got %+v", run)
	}
}

func TestGetLastCompletedRunUnparseableRunAt(t *testing.T) {
	database := openTestDB(t)
	insertRunRow(t, database, "run-bad", "not-a-time", "completed", "{}", 0, 0)
	if _, err := database.GetLastCompletedRun(); err == nil {
		t.Error("expected error for unparseable run_at, got nil")
	}
}

func TestMarkRunUndone(t *testing.T) {
	database := openTestDB(t)
	insertRunRow(t, database, "run-a", "2026-01-01 00:00:00", "completed", "{}", 0, 0)
	insertRunRow(t, database, "run-b", "2026-01-02 00:00:00", "completed", "{}", 0, 0)

	if err := database.MarkRunUndone("run-b"); err != nil {
		t.Fatalf("MarkRunUndone: %v", err)
	}
	run, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if run == nil || run.ID != "run-a" {
		t.Errorf("expected run-a to remain completed, got %+v", run)
	}

	// Marking a non-existent run is a no-op, not an error.
	if err := database.MarkRunUndone("does-not-exist"); err != nil {
		t.Errorf("MarkRunUndone on missing id: %v", err)
	}
}

func TestParseRunSummary(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		want    *RunSummary
		wantErr bool
	}{
		{
			name:  "full summary",
			input: `{"new_memory_ids":["n1","n2"],"archived_ids":["a1"],"replaced_to_new":{"a1":"n1"}}`,
			want: &RunSummary{
				NewMemoryIDs:  []string{"n1", "n2"},
				ArchivedIDs:   []string{"a1"},
				ReplacedToNew: map[string]string{"a1": "n1"},
			},
		},
		{name: "empty object", input: `{}`, want: &RunSummary{}},
		{name: "empty string", input: "", wantErr: true},
		{name: "not json", input: "not json", wantErr: true},
		{name: "truncated json", input: `{"new_memory_ids":["n1"]`, wantErr: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseRunSummary(tt.input)
			if (err != nil) != tt.wantErr {
				t.Fatalf("ParseRunSummary() error = %v, wantErr %v", err, tt.wantErr)
			}
			if tt.wantErr {
				return
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("ParseRunSummary() = %+v, want %+v", got, tt.want)
			}
		})
	}
}

func TestMarshalRunSummaryRoundTrip(t *testing.T) {
	rs := &RunSummary{
		NewMemoryIDs:  []string{"n1", "n2"},
		ArchivedIDs:   []string{"a1", "a2"},
		ReplacedToNew: map[string]string{"a1": "n1", "a2": "n2"},
	}
	encoded, err := MarshalRunSummary(rs)
	if err != nil {
		t.Fatalf("MarshalRunSummary: %v", err)
	}
	if !strings.Contains(encoded, `"new_memory_ids"`) || !strings.Contains(encoded, `"replaced_to_new"`) {
		t.Errorf("encoded summary missing fields: %s", encoded)
	}
	decoded, err := ParseRunSummary(encoded)
	if err != nil {
		t.Fatalf("ParseRunSummary: %v", err)
	}
	if !reflect.DeepEqual(decoded, rs) {
		t.Errorf("round trip mismatch:\n got %+v\nwant %+v", decoded, rs)
	}
}

func TestMarshalRunSummaryEmpty(t *testing.T) {
	encoded, err := MarshalRunSummary(&RunSummary{})
	if err != nil {
		t.Fatalf("MarshalRunSummary: %v", err)
	}
	decoded, err := ParseRunSummary(encoded)
	if err != nil {
		t.Fatalf("ParseRunSummary: %v", err)
	}
	if len(decoded.NewMemoryIDs) != 0 || len(decoded.ArchivedIDs) != 0 || len(decoded.ReplacedToNew) != 0 {
		t.Errorf("expected empty summary after round trip, got %+v", decoded)
	}
}

// TestMarkRunUndoneTx verifies the transaction-scoped variant updates the
// run status inside the caller's transaction.
func TestMarkRunUndoneTx(t *testing.T) {
	database := openTestDB(t)
	run := &ConsolidationRun{
		ID: "tx-undone", RunAt: time.Now().UTC(), Status: "completed",
		SummaryJSON: "{}", TotalArchived: 1, TotalConsolidated: 1,
	}
	if err := database.SaveConsolidationRun(run); err != nil {
		t.Fatalf("SaveConsolidationRun: %v", err)
	}

	tx, err := database.BeginTransaction()
	if err != nil {
		t.Fatalf("BeginTransaction: %v", err)
	}
	if err := database.MarkRunUndoneTx(tx, "tx-undone"); err != nil {
		t.Fatalf("MarkRunUndoneTx: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit: %v", err)
	}

	got, err := database.GetLastCompletedRun()
	if err != nil {
		t.Fatalf("GetLastCompletedRun: %v", err)
	}
	if got != nil {
		t.Errorf("expected no completed run after undo, got %+v", got)
	}
}
