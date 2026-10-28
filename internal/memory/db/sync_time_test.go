package db

import (
	"fmt"
	"reflect"
	"testing"
	"time"
)

func TestDeletedCursorTriggerTimestamps(t *testing.T) {
	database := oplogTestDB(t)
	for i := 0; i < 5; i++ {
		m := oplogMemory(fmt.Sprintf("trigger-%d", i), "trigger timestamp")
		if err := database.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
		if err := database.DeleteMemory(m.ID); err != nil {
			t.Fatal(err)
		}
	}
	since, lastID := time.Time{}, ""
	seen := map[string]bool{}
	for page := 0; page < 5; page++ {
		rows, err := database.GetDeletedSinceCursorID(since, lastID, 2)
		if err != nil {
			t.Fatal(err)
		}
		if len(rows) == 0 {
			if len(seen) != 5 {
				t.Fatalf("got %d tombstones, want 5", len(seen))
			}
			return
		}
		for _, row := range rows {
			if seen[row.ID] {
				t.Fatalf("page %d repeats tombstone %q", page, row.ID)
			}
			seen[row.ID] = true
		}
		since, lastID = rows[len(rows)-1].DeletedAt, rows[len(rows)-1].ID
	}
	t.Fatal("trigger timestamp pagination did not terminate")
}

func TestSyncCursorMixedTimestampLayouts(t *testing.T) {
	database := oplogTestDB(t)
	// IDs deliberately sort against time order. Include equivalent instants
	// with different layouts and adjacent nanoseconds that SQLite date
	// functions would round to the same millisecond.
	stamps := []string{
		"2026-01-01T00:00:00.100Z",
		"2026-01-01 00:00:00.100000001 +0000 UTC",
		"2026-01-01T01:00:00.100000001+01:00",
		"2026-01-01 00:00:00.100000002",
		"2026-01-01 00:00:00.2 +0000 UTC",
	}
	ids := []string{"z", "b", "c", "a", "y"}
	for i, stamp := range stamps {
		if _, err := database.conn.Exec("INSERT INTO sync_oplog(op, memory_id, ts) VALUES ('delete', ?, ?)", ids[i], stamp); err != nil {
			t.Fatal(err)
		}
		m := oplogMemory(ids[i], "mixed timestamp")
		if err := database.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
		if _, err := database.conn.Exec("UPDATE memories SET updated_at = ? WHERE id = ?", stamp, ids[i]); err != nil {
			t.Fatal(err)
		}
		// Both SaveMemory and UPDATE add upserts; keep a delete as latest.
		if _, err := database.conn.Exec("INSERT INTO sync_oplog(op, memory_id, ts) VALUES ('delete', ?, ?)", ids[i], stamp); err != nil {
			t.Fatal(err)
		}
		if _, err := database.conn.Exec("INSERT INTO sync_relay(id, updated_at, blob) VALUES (?, ?, ?)", ids[i], stamp, []byte("cipher")); err != nil {
			t.Fatal(err)
		}
	}
	for _, limit := range []int{1, 2, 10} {
		for _, stream := range []string{"delete", "memory", "sync_memory"} {
			t.Run(fmt.Sprintf("%s/limit-%d", stream, limit), func(t *testing.T) {
				since, lastID := time.Time{}, ""
				var got []string
				for page := 0; page < len(ids)+1; page++ {
					var batch []DeletedMemory
					var err error
					if stream == "delete" {
						batch, err = database.GetDeletedSinceCursorID(since, lastID, limit)
					} else {
						var memories []*Memory
						if stream == "memory" {
							memories, err = database.GetMemoriesSinceCursorID(since, lastID, limit)
						} else {
							memories, err = database.GetMemoriesSinceCursorIDForSync(since, lastID, limit)
						}
						for _, m := range memories {
							batch = append(batch, DeletedMemory{ID: m.ID, DeletedAt: m.UpdatedAt})
						}
					}
					if err != nil {
						t.Fatal(err)
					}
					if len(batch) == 0 {
						if !reflect.DeepEqual(got, ids) {
							t.Fatalf("got %v, want %v", got, ids)
						}
						return
					}
					for _, row := range batch {
						got = append(got, row.ID)
					}
					since, lastID = batch[len(batch)-1].DeletedAt, batch[len(batch)-1].ID
				}
				t.Fatalf("pagination did not terminate: %v", got)
			})
		}
	}
	// The initial timestamp-only predicate is strict across layouts too.
	cutoff := time.Date(2026, 1, 1, 0, 0, 0, 100000001, time.UTC)
	deleted, err := database.GetDeletedSince(cutoff)
	if err != nil || len(deleted) != 2 {
		t.Fatalf("deletes after cutoff=%v err=%v", deleted, err)
	}
	memories, err := database.GetMemoriesSinceCursor(cutoff, 10)
	if err != nil || len(memories) != 2 || memories[0].ID != "a" || memories[1].ID != "y" {
		t.Fatalf("memories after cutoff=%v err=%v", memories, err)
	}
	blobs, err := database.GetRelayBlobsSince(cutoff, 10)
	if err != nil || len(blobs) != 2 || blobs[0].ID != "a" || blobs[1].ID != "y" {
		t.Fatalf("blobs after cutoff=%v err=%v", blobs, err)
	}
}

func TestNormalizeSyncTime(t *testing.T) {
	for _, raw := range []string{
		"2026-01-01T00:00:00.123Z",
		"2026-01-01 00:00:00.123 +0000 UTC",
		"2026-01-01 01:00:00.123 +0100 CET",
		"2026-01-01T01:00:00.123+01:00",
		"2026-01-01 00:00:00.123",
		"2026-01-01 00:00:00.123 +0000 UTC m=+1.234",
	} {
		got, err := normalizeSyncTime(raw)
		if err != nil || got != "2026-01-01T00:00:00.123000000Z" {
			t.Errorf("normalize(%q)=%q,%v", raw, got, err)
		}
	}
	if _, err := normalizeSyncTime("not a timestamp"); err == nil {
		t.Fatal("malformed timestamp must fail instead of silently losing rows")
	}
}
