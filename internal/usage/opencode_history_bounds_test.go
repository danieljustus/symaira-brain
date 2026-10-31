//go:build unix

package usage

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestOpenCodeHistoryAdmissionBounds(t *testing.T) {
	for _, mode := range []string{"channels", "rows", "row-bytes", "symlink"} {
		t.Run(mode, func(t *testing.T) {
			root := t.TempDir()
			tmp := t.TempDir()
			t.Setenv("TMPDIR", tmp)
			if mode == "channels" {
				for i := 0; i < 9; i++ {
					if err := os.WriteFile(filepath.Join(root, fmt.Sprintf("opencode-%d.db", i)), nil, 0600); err != nil {
						t.Fatal(err)
					}
				}
			} else if mode == "symlink" {
				if err := os.Symlink(t.TempDir(), filepath.Join(root, "opencode.db")); err != nil {
					t.Fatal(err)
				}
			} else {
				db, err := sql.Open("sqlite", filepath.Join(root, "opencode.db"))
				if err != nil {
					t.Fatal(err)
				}
				_, err = db.Exec("CREATE TABLE message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT)")
				if err != nil {
					db.Close()
					t.Fatal(err)
				}
				query := "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10001) INSERT INTO message SELECT CAST(x AS TEXT),1,'{}' FROM n"
				if mode == "row-bytes" {
					query = "INSERT INTO message VALUES('oversized',1,replace(hex(zeroblob(131073)),'0','x'))"
				}
				_, err = db.Exec(query)
				if err != nil {
					db.Close()
					t.Fatal(err)
				}
				if err = db.Close(); err != nil {
					t.Fatal(err)
				}
			}
			h := ScanOpenCodeHistory(context.Background(), root, time.Now())
			if h.Status != "unavailable" || len(h.Daily) != 0 {
				t.Fatal(h)
			}
			if entries, err := os.ReadDir(tmp); err != nil || len(entries) != 0 {
				t.Fatal("private copy leak", err)
			}
		})
	}
}
