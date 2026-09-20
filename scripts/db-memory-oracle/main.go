package main

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

type SchemaTable struct {
	Name string `json:"name"`
	SQL  string `json:"sql"`
}

type SchemaColumn struct {
	Name         string      `json:"name"`
	Type         string      `json:"type"`
	NotNull      bool        `json:"not_null"`
	DefaultValue interface{} `json:"default_value"`
	PK           int         `json:"pk"`
}

type SchemaIndex struct {
	Name   string `json:"name"`
	Unique bool   `json:"unique"`
	SQL    string `json:"sql"`
}

type SchemaSnapshot struct {
	Tables   []SchemaTable  `json:"tables"`
	Memories []SchemaColumn `json:"memories_columns"`
	Indexes  []SchemaIndex  `json:"indexes"`
}

type NegativeCase struct {
	Name     string `json:"name"`
	SQL      string `json:"sql"`
	Rejected bool   `json:"rejected"`
	Error    string `json:"error,omitempty"`
}

type OrderingCase struct {
	Name        string   `json:"name"`
	ExpectedIDs []string `json:"expected_ids"`
}

type LockCase struct {
	Name              string `json:"name"`
	BusyTimeoutMillis int64  `json:"busy_timeout_ms"`
	JournalMode       string `json:"journal_mode"`
	ForeignKeys       int64  `json:"foreign_keys"`
	SecureDelete      int64  `json:"secure_delete"`
}

type Suite struct {
	Schema        SchemaSnapshot `json:"schema"`
	NegativeCases []NegativeCase `json:"negative_cases"`
	OrderingCases []OrderingCase `json:"ordering_cases"`
	Lock          LockCase       `json:"lock"`
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", "rust/symbrain-memory/tests/fixtures/db_memory_oracle.json", "output path")
	flag.Parse()

	suite, err := buildSuite()
	if err != nil {
		fmt.Fprintf(os.Stderr, "build oracle: %v\n", err)
		os.Exit(1)
	}

	data, err := json.MarshalIndent(suite, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "json marshal: %v\n", err)
		os.Exit(1)
	}
	data = append(data, '\n')

	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", *output, err)
			os.Exit(1)
		}
		if !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/db-memory-oracle -check\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: db-memory oracle deterministic check passed (%d tables, %d negative cases, %d ordering cases)\n", len(suite.Schema.Tables), len(suite.NegativeCases), len(suite.OrderingCases))
		return
	}

	if err := os.MkdirAll(filepath.Dir(*output), 0755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir %s: %v\n", filepath.Dir(*output), err)
		os.Exit(1)
	}
	if err := os.WriteFile(*output, data, 0644); err != nil {
		fmt.Fprintf(os.Stderr, "write %s: %v\n", *output, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s (%d tables, %d negative cases, %d ordering cases)\n", *output, len(suite.Schema.Tables), len(suite.NegativeCases), len(suite.OrderingCases))
}

func buildSuite() (Suite, error) {
	suite := Suite{}

	tempDir, err := os.MkdirTemp("", "symbrain-db-oracle-*")
	if err != nil {
		return suite, fmt.Errorf("temp dir: %w", err)
	}
	defer os.RemoveAll(tempDir)

	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "memory.db")
	database, err := db.Open(cfg)
	if err != nil {
		return suite, fmt.Errorf("open db: %w", err)
	}
	defer database.Close()

	conn := database.Conn()
	suite.Schema, err = captureSchema(conn)
	if err != nil {
		return suite, fmt.Errorf("capture schema: %w", err)
	}

	suite.NegativeCases = buildNegativeCases(conn, tempDir)
	suite.OrderingCases = buildOrderingCases(conn, tempDir)
	suite.Lock, err = captureLock(conn)
	if err != nil {
		return suite, fmt.Errorf("capture lock: %w", err)
	}

	return suite, nil
}

func captureSchema(conn *sql.DB) (SchemaSnapshot, error) {
	var snapshot SchemaSnapshot

	tableRows, err := conn.Query(`
		SELECT name, sql
		FROM sqlite_master
		WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
		ORDER BY name
	`)
	if err != nil {
		return snapshot, err
	}
	defer tableRows.Close()
	for tableRows.Next() {
		var table SchemaTable
		if err := tableRows.Scan(&table.Name, &table.SQL); err != nil {
			return snapshot, err
		}
		snapshot.Tables = append(snapshot.Tables, table)
	}
	if err := tableRows.Err(); err != nil {
		return snapshot, err
	}

	columnRows, err := conn.Query(`PRAGMA table_info(memories)`)
	if err != nil {
		return snapshot, err
	}
	defer columnRows.Close()
	for columnRows.Next() {
		var cid int
		var column SchemaColumn
		if err := columnRows.Scan(&cid, &column.Name, &column.Type, &column.NotNull, &column.DefaultValue, &column.PK); err != nil {
			return snapshot, err
		}
		snapshot.Memories = append(snapshot.Memories, column)
	}
	if err := columnRows.Err(); err != nil {
		return snapshot, err
	}

	indexRows, err := conn.Query(`PRAGMA index_list('memories')`)
	if err != nil {
		return snapshot, err
	}
	var indexNames []string
	for indexRows.Next() {
		var seq int
		var name string
		var uniqueInt int
		var origin string
		var partial string
		if err := indexRows.Scan(&seq, &name, &uniqueInt, &origin, &partial); err != nil {
			indexRows.Close()
			return snapshot, err
		}
		indexNames = append(indexNames, name)
	}
	indexRows.Close()
	if err := indexRows.Err(); err != nil {
		return snapshot, err
	}

	for _, name := range indexNames {
		var sqlStr *string
		err := conn.QueryRow(`SELECT sql FROM sqlite_master WHERE type='index' AND name=?`, name).Scan(&sqlStr)
		if err != nil {
			return snapshot, err
		}
		var index SchemaIndex
		index.Name = name
		if sqlStr != nil {
			index.SQL = *sqlStr
		}
		index.Unique = bytes.Contains([]byte(index.SQL), []byte("UNIQUE"))
		snapshot.Indexes = append(snapshot.Indexes, index)
	}

	return snapshot, nil
}

func buildNegativeCases(conn *sql.DB, tempDir string) []NegativeCase {
	cases := []NegativeCase{
		{Name: "null_created_at_rejected", SQL: "INSERT INTO memories (id, content, scope, created_at, updated_at) VALUES ('null-ts-1', 'content', 'global', NULL, '2024-01-01 00:00:00')"},
		{Name: "null_updated_at_rejected", SQL: "INSERT INTO memories (id, content, scope, created_at, updated_at) VALUES ('null-ts-2', 'content', 'global', '2024-01-01 00:00:00', NULL)"},
	}

	for i := range cases {
		_, err := conn.Exec(cases[i].SQL)
		cases[i].Rejected = err != nil
		if err != nil {
			cases[i].Error = err.Error()
		}
	}

	// Missing table: query a table that does not exist in the shipped schema.
	_, err := conn.Query(`SELECT * FROM nonexistent_table`)
	cases = append(cases, NegativeCase{
		Name:     "missing_table_rejected",
		SQL:      "SELECT * FROM nonexistent_table",
		Rejected: err != nil,
	})
	if err != nil {
		cases[len(cases)-1].Error = err.Error()
	}

	// Legacy schema: a database created by an older version missing the
	// `embedding_dim` column should still be readable after migration parity.
	legacyPath := filepath.Join(tempDir, "legacy.db")
	legacyConn, err := sql.Open("sqlite3", legacyPath)
	if err == nil {
		_, _ = legacyConn.Exec(`CREATE TABLE memories (id TEXT PRIMARY KEY, content TEXT NOT NULL, scope TEXT NOT NULL, metadata TEXT NOT NULL, embedding TEXT NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL)`)
		_, _ = legacyConn.Exec(`INSERT INTO memories (id, content, scope, metadata, embedding, created_at, updated_at) VALUES ('legacy-1', 'legacy content', 'global', '{}', '[]', '2024-01-01 00:00:00', '2024-01-01 00:00:00')`)
		legacyConn.Close()

		// Re-open through the production DB package so migrations run.
		legacyCfg := config.Defaults()
		legacyCfg.Database.Path = legacyPath
		legacyDB, openErr := db.Open(legacyCfg)
		if openErr == nil {
			defer legacyDB.Close()
			_, err := legacyDB.Conn().Query(`SELECT embedding_dim FROM memories WHERE id = 'legacy-1'`)
			cases = append(cases, NegativeCase{
				Name:     "legacy_schema_migrated",
				SQL:      "SELECT embedding_dim FROM memories WHERE id = 'legacy-1'",
				Rejected: err != nil,
			})
			if err != nil {
				cases[len(cases)-1].Error = err.Error()
			}
		}
	}

	return cases
}

func buildOrderingCases(conn *sql.DB, tempDir string) []OrderingCase {
	cases := []OrderingCase{}

	// Insert rows with identical timestamps to exercise tie-breaking by id DESC.
	_, _ = conn.Exec(`INSERT INTO memories (id, content, scope, created_at, updated_at) VALUES
		('tie-a', 'content-a', 'global', '2024-06-01 12:00:00', '2024-06-01 12:00:00'),
		('tie-b', 'content-b', 'global', '2024-06-01 12:00:00', '2024-06-01 12:00:00'),
		('tie-c', 'content-c', 'global', '2024-06-01 12:00:00', '2024-06-01 12:00:00')`)

	rows, err := conn.Query(`SELECT id FROM memories WHERE id LIKE 'tie-%' ORDER BY created_at DESC, id DESC`)
	if err == nil {
		var ids []string
		for rows.Next() {
			var id string
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				break
			}
			ids = append(ids, id)
		}
		rows.Close()
		cases = append(cases, OrderingCase{Name: "ordering_ties_by_id_desc", ExpectedIDs: ids})
	}

	// Insert rows with distinct timestamps to verify created_at DESC ordering.
	_, _ = conn.Exec(`INSERT INTO memories (id, content, scope, created_at, updated_at) VALUES
		('order-old', 'content-old', 'global', '2024-01-01 00:00:00', '2024-01-01 00:00:00'),
		('order-new', 'content-new', 'global', '2024-12-31 23:59:59', '2024-12-31 23:59:59')`)

	rows, err = conn.Query(`SELECT id FROM memories WHERE id LIKE 'order-%' ORDER BY created_at DESC, id DESC`)
	if err == nil {
		var ids []string
		for rows.Next() {
			var id string
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				break
			}
			ids = append(ids, id)
		}
		rows.Close()
		cases = append(cases, OrderingCase{Name: "ordering_distinct_by_created_at_desc", ExpectedIDs: ids})
	}

	return cases
}

func captureLock(conn *sql.DB) (LockCase, error) {
	lock := LockCase{Name: "single_connection_wal_foreign_keys_secure_delete"}

	var journalMode string
	if err := conn.QueryRow(`PRAGMA journal_mode`).Scan(&journalMode); err != nil {
		return lock, err
	}
	lock.JournalMode = journalMode

	var foreignKeys int64
	if err := conn.QueryRow(`PRAGMA foreign_keys`).Scan(&foreignKeys); err != nil {
		return lock, err
	}
	lock.ForeignKeys = foreignKeys

	var secureDelete int64
	if err := conn.QueryRow(`PRAGMA secure_delete`).Scan(&secureDelete); err != nil {
		return lock, err
	}
	lock.SecureDelete = secureDelete

	var busyTimeout int64
	if err := conn.QueryRow(`PRAGMA busy_timeout`).Scan(&busyTimeout); err != nil {
		return lock, err
	}
	lock.BusyTimeoutMillis = busyTimeout

	return lock, nil
}

var _ = time.Now
