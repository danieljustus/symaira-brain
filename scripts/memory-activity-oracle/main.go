// Command memory-activity-oracle snapshots the Go-owned SQLite and MCP
// contracts using an isolated temporary database. Keep this source-bound: it
// deliberately uses the production db/activity packages, not a hand-written
// second implementation.
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

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	memorydb "github.com/danieljustus/symaira-brain/internal/memory/db"
)

type snapshot struct {
	Pragmas    map[string]string `json:"pragmas"`
	Migrations int               `json:"migrations"`
	NullOrder  []string          `json:"null_order"`
	LockResult string            `json:"lock_result"`
	Activity   []string          `json:"activity_validation"`
	Catalog    []string          `json:"catalog"`
	ToolErrors []string          `json:"tool_errors"`
}

func main() {
	check := flag.Bool("check", false, "compare output with a fixture")
	output := flag.String("output", "rust/symbrain-memory/tests/fixtures/go_oracle.json", "fixture path")
	flag.Parse()
	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		panic(err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/memory-activity-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: memory/activity SQLite/MCP oracle is deterministic")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		panic(err)
	}
	fmt.Printf("Wrote %s\n", *output)
}

func generate() snapshot {
	dir, err := os.MkdirTemp("", "memory-activity-oracle-")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	cfg := memoryconfig.Defaults()
	cfg.Database.Path = filepath.Join(dir, "oracle.db")
	db, err := memorydb.Open(cfg)
	if err != nil {
		panic(err)
	}
	defer db.Close()

	pragmas := map[string]string{}
	for _, name := range []string{"foreign_keys", "secure_delete", "journal_mode"} {
		var value string
		if err := db.Conn().QueryRow("PRAGMA " + name).Scan(&value); err != nil {
			panic(err)
		}
		pragmas[name] = value
	}
	var migrations int
	if err := db.Conn().QueryRow("SELECT COUNT(*) FROM schema_migrations").Scan(&migrations); err != nil {
		panic(err)
	}
	nullOrder := nullTimestampOrder(db.Conn())
	lockResult := lockProbe(db.Conn())
	base := time.Date(2026, 8, 1, 9, 0, 0, 0, time.UTC)
	valid := activity.ValidateSearchOptions(activity.SearchOptions{Query: "x", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 1})
	invalid := activity.ValidateSearchOptions(activity.SearchOptions{Query: " ", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 1})
	activityValidation := []string{"valid"}
	if valid != nil {
		activityValidation[0] = valid.Error()
	}
	activityValidation = append(activityValidation, "query-required="+invalid.Error())
	return snapshot{
		Pragmas: pragmas, Migrations: migrations, NullOrder: nullOrder,
		LockResult: lockResult, Activity: activityValidation,
		Catalog:    []string{"activity_get", "activity_search", "activity_status", "entity_list", "memory_get", "memory_list", "memory_search", "memory_set", "query_log"},
		ToolErrors: []string{"activity_search: limit and max_tokens are required; unbounded queries are refused", "memory_set: content and kind are required"},
	}
}

func nullTimestampOrder(conn *sql.DB) []string {
	if _, err := conn.Exec("CREATE TEMP TABLE oracle_order (id TEXT, ts TEXT)"); err != nil {
		panic(err)
	}
	if _, err := conn.Exec("INSERT INTO oracle_order VALUES ('null', NULL), ('late', '2026-08-02T00:00:00Z'), ('early', '2026-08-01T00:00:00Z')"); err != nil {
		panic(err)
	}
	rows, err := conn.Query("SELECT id FROM oracle_order ORDER BY ts ASC, id ASC")
	if err != nil {
		panic(err)
	}
	defer rows.Close()
	var result []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			panic(err)
		}
		result = append(result, id)
	}
	if err := rows.Err(); err != nil {
		panic(err)
	}
	return result
}

func lockProbe(conn *sql.DB) string {
	if _, err := conn.Exec("BEGIN IMMEDIATE"); err != nil {
		return err.Error()
	}
	if _, err := conn.Exec("ROLLBACK"); err != nil {
		panic(err)
	}
	return "begin-immediate-ok"
}
