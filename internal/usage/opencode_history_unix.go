//go:build unix

package usage

import (
	"context"
	"database/sql"
	"io"
	"os"
	"sort"
	"strings"
)

func readOpenCodeHistory(ctx context.Context, dir string) ([]historyEvent, int, error) {
	if ctx.Err() != nil {
		return nil, 0, ctx.Err()
	}
	root, err := openCodeDirectory(dir)
	if os.IsNotExist(err) {
		return nil, 0, nil
	}
	if err != nil {
		return nil, 0, err
	}
	defer root.Close()
	entries, err := root.ReadDir(4097)
	if err != nil && err != io.EOF {
		return nil, 0, err
	}
	if len(entries) > 4096 {
		return nil, 0, os.ErrInvalid
	}
	names := []string{}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "opencode") && strings.HasSuffix(entry.Name(), ".db") {
			names = append(names, entry.Name())
		}
	}
	if len(names) > 8 {
		return nil, 0, os.ErrInvalid
	}
	sort.Strings(names)
	rows := []openCodeHistoryRow{}
	excluded := 0
	bytes := 0
	for _, name := range names {
		if ctx.Err() != nil {
			return nil, 0, ctx.Err()
		}
		err := withOpenCodeDatabase(ctx, dir, name, func(ctx context.Context, db *sql.DB) error {
			for _, table := range []string{"message", "session_message"} {
				var count, valid int
				if db.QueryRowContext(ctx, "SELECT count(*), coalesce(sum(type='table'),0) FROM sqlite_schema WHERE name=?", table).Scan(&count, &valid) != nil || count != valid {
					return os.ErrInvalid
				}
				if count == 0 {
					continue
				}
				kind := "''"
				if table == "session_message" {
					kind = "coalesce(type,'')"
				}
				// Bound all source rows (including rejected ownership) before parsing; never evaluate source JSON in SQL.
				query := "SELECT coalesce(CASE WHEN typeof(id)='text' THEN substr(id,1,257) END,''), CASE WHEN typeof(time_created)='integer' THEN time_created ELSE 0 END, " + kind + ", coalesce(CASE WHEN typeof(data)='text' THEN substr(data,1,262145) END,''), typeof(id), typeof(time_created), typeof(data) FROM " + table + " LIMIT 10001"
				result, err := db.QueryContext(ctx, query)
				if err != nil {
					return err
				}
				for result.Next() {
					var id, typ, idType, timeType, dataType string
					var ms int64
					var data []byte
					if result.Scan(&id, &ms, &typ, &data, &idType, &timeType, &dataType) != nil {
						result.Close()
						return os.ErrInvalid
					}
					bytes += len(data)
					if len(rows) >= 10000 || len(data) > 262144 || bytes > 32<<20 {
						result.Close()
						return os.ErrInvalid
					}
					// Count ignored and malformed rows too: rows is also the scan budget counter.
					row, ok := parseOpenCodeHistoryRow(id, ms, typ, data)
					if idType != "text" || timeType != "integer" || dataType != "text" {
						ok = false
					}
					if !ok {
						if idType == "text" && len(id) > 0 && len(id) <= 256 {
							row = openCodeHistoryRow{id: id}
						}
						excluded++
					}
					rows = append(rows, row)
				}
				err = result.Err()
				closeErr := result.Close()
				if err != nil {
					return err
				}
				if closeErr != nil {
					return closeErr
				}
			}
			return nil
		})
		if err != nil {
			return nil, 0, err
		}
	}
	if ctx.Err() != nil {
		return nil, 0, ctx.Err()
	}
	events, excluded := dedupOpenCodeHistory(rows, excluded)
	return events, excluded, nil
}
