//go:build unix

package usage

import (
	"context"
	"database/sql"
	"testing"
	"time"
)

func TestOpenCodeSQLContextCancellation(t *testing.T) {
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if _, err := db.Exec(openCodeTestSchema + `INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1);`); err != nil {
		t.Fatal(err)
	}
	for _, expired := range []bool{false, true} {
		var ctx context.Context
		var cancel context.CancelFunc
		if expired {
			ctx, cancel = context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
		} else {
			ctx, cancel = context.WithCancel(context.Background())
			cancel()
		}
		key, authoritative, err := openCodeCredentialRow(ctx, db)
		cancel()
		if err == nil || key != "" || !authoritative {
			t.Fatal("cancelled SQL must fail closed")
		}
	}
}
