package db

import (
	"embed"

	"github.com/danieljustus/symaira-corekit/sqlitekit"
)

//go:embed migrations/*.sql
var migrationFS embed.FS

func (db *DB) runMigrations() error {
	return sqlitekit.Migrate(db.conn, migrationFS)
}
