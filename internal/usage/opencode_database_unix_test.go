//go:build unix

package usage

import (
	"database/sql"
	"os"
	"path/filepath"
	"testing"
)

const openCodeTestSchema = `CREATE TABLE migration(id TEXT PRIMARY KEY,time_completed INTEGER);
CREATE TABLE credential(id TEXT PRIMARY KEY,integration_id TEXT,label TEXT,value TEXT,connector_id TEXT,method_id TEXT,active INTEGER,time_created INTEGER,time_updated INTEGER);`

func TestOpenCodeV2ActiveCheckpoint(t *testing.T) {
	dir := t.TempDir()
	db, err := sql.Open("sqlite", filepath.Join(dir, "opencode.db"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = db.Exec(openCodeTestSchema + `INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1);
 INSERT INTO credential(id,integration_id,value,active,time_updated) VALUES('current','opencode-go','{"key":"synthetic-current"}',1,1);`); err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "auth.json"), []byte(`{"opencode-go":{"key":"stale"}}`), 0600); err != nil {
		t.Fatal(err)
	}
	key, err := openCodeAuthJSONKey(dir)
	if err != nil || key != "synthetic-current" {
		t.Fatal("current DB row not selected")
	}
}
