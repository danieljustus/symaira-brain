//go:build unix

package usage

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func TestOpenCodeHistoryWALAndCleanup(t *testing.T) {
	root := t.TempDir()
	tmp := t.TempDir()
	t.Setenv("TMPDIR", tmp)
	db, err := sql.Open("sqlite", filepath.Join(root, "opencode.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if _, err = db.Exec("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT)"); err != nil {
		t.Fatal(err)
	}
	data := `{"role":"assistant","providerID":"opencode-go","modelID":"go-model","cost":0,"tokens":{"total":0}}`
	if _, err = db.Exec("INSERT INTO message VALUES(?,?,?)", "wal-event", time.Now().Add(-time.Second).UnixMilli(), data); err != nil {
		t.Fatal(err)
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
	if got := ScanOpenCodeHistory(context.Background(), root, time.Now()); got.Status != "available" {
		t.Fatal(got)
	}
	if !reflect.DeepEqual(before, inventory()) {
		t.Fatal("WAL source changed")
	}
	if entries, err := os.ReadDir(tmp); err != nil || len(entries) != 0 {
		t.Fatal("private copy leak", err)
	}
	// SQL error occurs after private-copy creation; source and temporary inventory still stay intact.
	if _, err = db.Exec("DROP TABLE message; CREATE TABLE message(unrecognized TEXT)"); err != nil {
		t.Fatal(err)
	}
	before = inventory()
	if got := ScanOpenCodeHistory(context.Background(), root, time.Now()); got.Status != "unavailable" || len(got.Daily) != 0 {
		t.Fatal(got)
	}
	if !reflect.DeepEqual(before, inventory()) {
		t.Fatal("error mutated source")
	}
	if entries, err := os.ReadDir(tmp); err != nil || len(entries) != 0 {
		t.Fatal("error leaked private copy", err)
	}
}

func TestOpenCodeHistoryIndependentQuotaError(t *testing.T) {
	root := t.TempDir()
	t.Setenv("OPENCODE_DATA_DIR", root)
	t.Setenv("SYMBRAIN_USAGE_OPENCODE_HISTORY", "1")
	db, err := sql.Open("sqlite", filepath.Join(root, "opencode.db"))
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec("CREATE TABLE message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT)")
	if err != nil {
		db.Close()
		t.Fatal(err)
	}
	_, err = db.Exec("INSERT INTO message VALUES(?,?,?)", "test", time.Now().Add(-time.Second).UnixMilli(), `{"role":"assistant","providerID":"opencode","modelID":"m","cost":0,"tokens":{"total":0}}`)
	if err != nil {
		db.Close()
		t.Fatal(err)
	}
	db.Close()
	p := &openCodeCurrentProvider{OpenCodeProvider: &OpenCodeProvider{client: &httpClientHistory}, apiKey: "synthetic"}
	got := BuildReport(context.Background(), []Provider{selectedHistoryProvider{p}}).Providers[0]
	if got.History == nil || got.History.Status != "available" || got.Error == "" || got.Snapshot != nil {
		t.Fatal(got)
	}
	// Merely including OpenCode in the default graph is not explicit selection.
	if got := BuildReport(context.Background(), []Provider{p}).Providers[0]; got.History != nil {
		t.Fatal("default history activated")
	}
	for _, flag := range []string{"", "0", "true"} {
		t.Setenv("SYMBRAIN_USAGE_OPENCODE_HISTORY", flag)
		if got := BuildReport(context.Background(), []Provider{selectedHistoryProvider{p}}).Providers[0]; got.History != nil {
			t.Fatal("nonexact flag activated history")
		}
	}
}
