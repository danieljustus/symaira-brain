//go:build unix

package usage

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func TestOpenCodeRecordedHistoryShared(t *testing.T) {
	raw, err := os.ReadFile("testdata/opencode-history-cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name string `json:"name"`
		Now  string `json:"now"`
		Rows []struct {
			DB    string `json:"db"`
			Table string `json:"table"`
			ID    string `json:"id"`
			MS    int64  `json:"ms"`
			Kind  string `json:"kind"`
			Data  string `json:"data"`
		} `json:"rows"`
		Expected json.RawMessage `json:"expected"`
	}
	if err = json.Unmarshal(raw, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			root := t.TempDir()
			tmp := t.TempDir()
			t.Setenv("TMPDIR", tmp)
			for _, row := range c.Rows {
				db, err := sql.Open("sqlite", filepath.Join(root, row.DB))
				if err != nil {
					t.Fatal(err)
				}
				_, err = db.Exec("CREATE TABLE IF NOT EXISTS message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT); CREATE TABLE IF NOT EXISTS session_message(id TEXT PRIMARY KEY,time_created INTEGER,type TEXT,data TEXT)")
				if err != nil {
					db.Close()
					t.Fatal(err)
				}
				if row.Table == "message" {
					_, err = db.Exec("INSERT INTO message VALUES(?,?,?)", row.ID, row.MS, row.Data)
				} else {
					_, err = db.Exec("INSERT INTO session_message VALUES(?,?,?,?)", row.ID, row.MS, row.Kind, row.Data)
				}
				if err != nil {
					db.Close()
					t.Fatal(err)
				}
				if err = db.Close(); err != nil {
					t.Fatal(err)
				}
			}
			inventory := func() map[string]string {
				t.Helper()
				entries, err := os.ReadDir(root)
				if err != nil {
					t.Fatal(err)
				}
				out := map[string]string{}
				for _, e := range entries {
					b, err := os.ReadFile(filepath.Join(root, e.Name()))
					if err != nil {
						t.Fatal(err)
					}
					out[e.Name()] = string(b)
				}
				return out
			}
			before := inventory()
			now, err := time.Parse(time.RFC3339, c.Now)
			if err != nil {
				t.Fatal(err)
			}
			h := ScanOpenCodeHistory(context.Background(), root, now)
			actual, err := json.Marshal(h)
			if err != nil {
				t.Fatal(err)
			}
			var a, b any
			json.Unmarshal(actual, &a)
			json.Unmarshal(c.Expected, &b)
			if !reflect.DeepEqual(a, b) {
				t.Fatalf("history differs: %s expected %s", actual, c.Expected)
			}
			if !reflect.DeepEqual(before, inventory()) {
				t.Fatal("source inventory changed")
			}
			entries, err := os.ReadDir(tmp)
			if err != nil || len(entries) != 0 {
				t.Fatal("private copy leak", err)
			}
			ctx, cancel := context.WithCancel(context.Background())
			cancel()
			if got := ScanOpenCodeHistory(ctx, root, now); got.Status != "unavailable" {
				t.Fatal("cancelled scan admitted")
			}
		})
	}
}
