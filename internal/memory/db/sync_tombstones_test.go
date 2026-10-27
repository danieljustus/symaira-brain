package db

import (
	"reflect"
	"testing"
	"time"
)

func TestDeletedCursorOnlyLatestDeletes(t *testing.T) {
	database := oplogTestDB(t)
	base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	for _, event := range []struct {
		id, op string
		offset time.Duration
	}{
		{"a", "delete", 0}, {"b", "delete", 0}, {"c", "delete", 0},
		{"b", "upsert", 0}, {"d", "upsert", time.Second}, {"e", "delete", time.Second},
	} {
		if _, err := database.conn.Exec("INSERT INTO sync_oplog (memory_id, op, ts) VALUES (?, ?, ?)", event.id, event.op, base.Add(event.offset)); err != nil {
			t.Fatal(err)
		}
	}
	for _, limit := range []int{1, 2, 10} {
		since, last := base.Add(-time.Second), ""
		var ids []string
		for page := 0; page < 10; page++ {
			got, err := database.GetDeletedSinceCursorID(since, last, limit)
			if err != nil {
				t.Fatal(err)
			}
			if len(got) == 0 {
				break
			}
			for _, d := range got {
				ids = append(ids, d.ID)
			}
			since, last = got[len(got)-1].DeletedAt, got[len(got)-1].ID
		}
		if !reflect.DeepEqual(ids, []string{"a", "c", "e"}) {
			t.Errorf("limit %d: deletes = %v", limit, ids)
		}
	}
}

func TestRemoteTombstoneOrderingAndResurrection(t *testing.T) {
	for _, present := range []bool{false, true} {
		for _, offset := range []time.Duration{-time.Second, 0, time.Second} {
			t.Run(time.Duration(offset).String()+map[bool]string{true: "/present", false: "/absent"}[present], func(t *testing.T) {
				database := oplogTestDB(t)
				deletedAt := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
				m := oplogMemory("remote-id", "remote content")
				m.CreatedAt, m.UpdatedAt = deletedAt.Add(-time.Hour), deletedAt.Add(-time.Hour)
				if present {
					if _, err := database.UpsertMemoryIfNewer(m); err != nil {
						t.Fatal(err)
					}
				}
				removed, err := database.ApplyRemoteDelete(m.ID, deletedAt)
				if err != nil || removed != present {
					t.Fatalf("removed=%v err=%v", removed, err)
				}
				// Replayed and older deletes must not move the deletion ordering backwards.
				for _, ts := range []time.Time{deletedAt, deletedAt.Add(-time.Minute)} {
					if removed, err := database.ApplyRemoteDelete(m.ID, ts); err != nil || removed {
						t.Fatalf("replay removed=%v err=%v", removed, err)
					}
				}
				ts, ok, err := database.GetTombstone(m.ID)
				if err != nil || !ok || !ts.Equal(deletedAt) {
					t.Errorf("tombstone=%v,%v,%v want %v", ts, ok, err, deletedAt)
				}
				page, err := database.GetDeletedSinceCursorID(deletedAt.Add(-time.Second), "", 10)
				if err != nil || len(page) != 1 || !page[0].DeletedAt.Equal(deletedAt) {
					t.Fatalf("outgoing tombstone=%v err=%v", page, err)
				}
				m.UpdatedAt = deletedAt.Add(offset)
				applied, err := database.SyncUpsertMemoryIfNewer(m)
				if err != nil || applied != (offset > 0) {
					t.Errorf("upsert applied=%v err=%v offset=%v", applied, err, offset)
				}
			})
		}
	}
}
