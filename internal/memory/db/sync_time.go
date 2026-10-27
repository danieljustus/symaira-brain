package db

import (
	"database/sql/driver"
	"fmt"
	"strings"
	"sync"
	"time"

	"modernc.org/sqlite"
)

var registerSyncTime = sync.OnceValue(func() error {
	return sqlite.RegisterDeterministicScalarFunction("symbrain_sync_time", 1,
		func(_ *sqlite.FunctionContext, args []driver.Value) (driver.Value, error) {
			raw, ok := args[0].(string)
			if !ok {
				return nil, fmt.Errorf("sync timestamp is not text")
			}
			return normalizeSyncTime(raw)
		})
})

// normalizeSyncTime makes driver-written, trigger-written and legacy SQLite
// timestamps comparable without changing stored data. Fixed-width UTC text
// preserves nanoseconds (unlike julianday/strftime) and sorts chronologically.
func normalizeSyncTime(raw string) (string, error) {
	// Match modernc's support for time.String(), including a monotonic suffix.
	text, _, _ := strings.Cut(raw, " m=")
	// time.String() appends the zone abbreviation, which is numeric (for
	// example "+0100") for unnamed fixed zones and cannot be parsed by an
	// "MST" layout. The numeric offset before it is authoritative.
	if f := strings.Fields(text); len(f) == 4 {
		if ts, err := time.Parse("2006-01-02 15:04:05.999999999 -0700", strings.Join(f[:3], " ")); err == nil {
			return ts.UTC().Format("2006-01-02T15:04:05.000000000Z"), nil
		}
	}
	for _, layout := range []string{
		"2006-01-02 15:04:05.999999999 -0700 MST",
		time.RFC3339Nano,
		"2006-01-02 15:04:05.999999999-07:00",
		"2006-01-02 15:04:05.999999999",
		"2006-01-02T15:04:05.999999999",
		"2006-01-02 15:04",
		"2006-01-02T15:04",
		"2006-01-02",
	} {
		if ts, err := time.Parse(layout, strings.TrimSuffix(strings.TrimSpace(text), "Z")); err == nil {
			return ts.UTC().Format("2006-01-02T15:04:05.000000000Z"), nil
		}
	}
	return "", fmt.Errorf("invalid sync timestamp %q", raw)
}
