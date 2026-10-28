//go:build unix

package usage

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"golang.org/x/sys/unix"
	_ "modernc.org/sqlite"
)

var errOpenCodeDatabase = errors.New("OpenCode database unreadable or ambiguous; set OPENCODE_GO_API_KEY explicitly")

// Keep a directory capability throughout admission; no symlinked ancestor or FIFO is opened.
func openCodeDirectory(path string) (*os.File, error) {
	path, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	fd, err := unix.Open("/", unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	for _, part := range strings.Split(strings.TrimPrefix(path, "/"), "/") {
		if part == "" {
			continue
		}
		next, e := unix.Openat(fd, part, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
		_ = unix.Close(fd)
		if e != nil {
			return nil, e
		}
		fd = next
	}
	return os.NewFile(uintptr(fd), "OpenCode directory"), nil
}

func openCodeDBFile(dir *os.File, name string, limit int64) (data []byte, err error) {
	fd, err := unix.Openat(int(dir.Fd()), name, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(fd), "OpenCode database input")
	defer func() { err = errors.Join(err, f.Close()) }()
	info, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return nil, os.ErrInvalid
	}
	data, err = io.ReadAll(io.LimitReader(f, limit+1))
	if int64(len(data)) > limit {
		return nil, os.ErrInvalid
	}
	return data, err
}

// SQLite only opens a private copy. Immutable mode would silently ignore a live WAL.
// A changing source fails closed rather than combining checkpoint generations.
func openCodeDatabaseKey(dir, name string) (key string, authoritative bool, err error) {
	fail := func() (string, bool, error) { return "", true, errOpenCodeDatabase }
	root, err := openCodeDirectory(dir)
	if err != nil {
		return fail()
	}
	defer root.Close()
	names := []string{name, name + "-wal", name + "-shm", name + "-journal"}
	limits := []int64{64 << 20, 16 << 20, 1 << 20, 0}
	inputs := make([][]byte, len(names))
	for i, n := range names {
		data, e := openCodeDBFile(root, n, limits[i])
		if os.IsNotExist(e) && i > 0 {
			continue
		}
		if e != nil || i == 3 {
			return fail()
		}
		inputs[i] = data
	}
	if len(inputs[0]) == 0 || !openCodeValidWAL(inputs[0], inputs[1]) {
		return fail()
	}
	for i, n := range names {
		data, e := openCodeDBFile(root, n, limits[i])
		if os.IsNotExist(e) && inputs[i] == nil {
			continue
		}
		if e != nil || !bytes.Equal(data, inputs[i]) {
			return fail()
		}
	}
	tmp, err := os.MkdirTemp("", "symbrain-opencode-")
	if err != nil {
		return fail()
	}
	defer os.RemoveAll(tmp)
	for i := 0; i < 2; i++ {
		if inputs[i] != nil {
			if os.WriteFile(filepath.Join(tmp, names[i]), inputs[i], 0600) != nil {
				return fail()
			}
		}
	}
	u := url.URL{Scheme: "file", Path: filepath.Join(tmp, name), RawQuery: "mode=ro"}
	db, err := sql.Open("sqlite", u.String())
	if err != nil {
		return fail()
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if _, err = db.ExecContext(ctx, "PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;"); err != nil {
		return fail()
	}
	var integrity string
	if db.QueryRowContext(ctx, "PRAGMA quick_check(1)").Scan(&integrity) != nil || integrity != "ok" {
		return fail()
	}
	key, authoritative, err = openCodeCredentialRow(ctx, db)
	if err != nil {
		return fail()
	}
	return key, authoritative, nil
}

func openCodeCredentialRow(ctx context.Context, db *sql.DB) (string, bool, error) {
	// Table identity and schema are evidence, not exception-message matching.
	table := func(name string) (bool, error) {
		var count, valid int
		err := db.QueryRowContext(ctx, "SELECT count(*), coalesce(sum(type='table'),0) FROM sqlite_schema WHERE name=?", name).Scan(&count, &valid)
		if count != valid {
			return false, errOpenCodeDatabase
		}
		return count == 1, err
	}
	schema := func(name string, columns map[string]string) bool {
		for col, typ := range columns {
			var n int
			if db.QueryRowContext(ctx, "SELECT count(*) FROM pragma_table_info(?) WHERE name=? AND upper(type)=? AND (name!='id' OR pk=1)", name, col, typ).Scan(&n) != nil || n != 1 {
				return false
			}
		}
		return true
	}
	hasMigration, err := table("migration")
	if err != nil || !hasMigration || !schema("migration", map[string]string{"id": "TEXT", "time_completed": "INTEGER"}) {
		return "", true, errOpenCodeDatabase
	}
	var imported int
	if err = db.QueryRowContext(ctx, "SELECT count(*) FROM migration WHERE id='20260805200742_import_legacy_credentials'").Scan(&imported); err != nil || imported > 1 {
		return "", true, errOpenCodeDatabase
	}
	hasCredential, err := table("credential")
	if err != nil {
		return "", true, err
	}
	if !hasCredential {
		if imported != 0 {
			return "", true, errOpenCodeDatabase
		}
		return "", false, nil
	}
	if !schema("credential", map[string]string{"id": "TEXT", "integration_id": "TEXT", "value": "TEXT", "active": "INTEGER", "time_updated": "INTEGER"}) {
		return "", true, errOpenCodeDatabase
	}
	// Prepare even before import, so a damaged credential schema cannot enable fallback.
	rows, err := db.QueryContext(ctx, `SELECT substr(id,1,1025), active, time_updated, substr(value,1,65537), typeof(value), CASE WHEN typeof(id)='text' AND typeof(time_updated)='integer' AND typeof(active) IN ('null','integer') THEN 1 ELSE 0 END
 FROM credential WHERE integration_id='opencode-go'
 ORDER BY active DESC, time_updated DESC, id DESC LIMIT 257`)
	if err != nil {
		return "", true, err
	}
	defer rows.Close()
	// The upstream order identifies the current account, not the first usable key.
	var selected []byte
	count := 0
	for rows.Next() {
		var id, kind string
		var typed int
		var active, updated sql.NullInt64
		var value []byte
		if rows.Scan(&id, &active, &updated, &value, &kind, &typed) != nil {
			return "", true, errOpenCodeDatabase
		}
		count++
		if typed != 1 || count > 256 || id == "" || len(id) > 1024 || !updated.Valid || updated.Int64 < 0 || (active.Valid && active.Int64 != 0 && active.Int64 != 1) || kind != "text" || len(value) > 65536 {
			return "", true, errOpenCodeDatabase
		}
		if active.Valid && active.Int64 == 0 {
			continue
		}
		if selected == nil {
			selected = value
		}
	}
	if rows.Close() != nil {
		return "", true, errOpenCodeDatabase
	}
	if rows.Err() != nil {
		return "", true, errOpenCodeDatabase
	}
	if imported == 0 {
		var total int
		if db.QueryRowContext(ctx, "SELECT count(*) FROM credential").Scan(&total) != nil || total != 0 {
			return "", true, errOpenCodeDatabase
		}
		return "", false, nil
	}
	if selected == nil {
		return "", true, nil
	}
	var value map[string]json.RawMessage
	if json.Unmarshal(selected, &value) != nil || value == nil {
		return "", true, errOpenCodeDatabase
	}
	raw, ok := value["key"]
	if !ok {
		return "", true, nil
	}
	var key string
	if json.Unmarshal(raw, &key) != nil || string(raw) == "null" {
		return "", true, errOpenCodeDatabase
	}
	if strings.TrimSpace(key) == "" {
		return "", true, nil
	}
	if !validOpenCodeKey(key) {
		return "", true, errOpenCodeDatabase
	}
	return strings.TrimSpace(key), true, nil
}
