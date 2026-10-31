//go:build unix

package usage

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"
)

func openCodeInputInventory(t *testing.T, dir string) map[string]string {
	t.Helper()
	result := map[string]string{}
	err := filepath.WalkDir(dir, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return nil
		}
		value := fmt.Sprintf("%v:%d:%d", info.Mode(), info.Size(), info.ModTime().UnixNano())
		if info.Mode().IsRegular() {
			data, e := os.ReadFile(path)
			if e != nil {
				return e
			}
			value += fmt.Sprintf(":%x", sha256.Sum256(data))
		}
		if info.Mode()&os.ModeSymlink != 0 {
			link, e := os.Readlink(path)
			if e != nil {
				return e
			}
			value += ":" + link
		}
		result[path] = value
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return result
}

func TestOpenCodeV2FreshProcesses(t *testing.T) {
	if dir := os.Getenv("OPENCODE_V2_CHILD_DIR"); dir != "" {
		key, err := openCodeAuthJSONKey(dir)
		switch os.Getenv("OPENCODE_V2_OUTCOME") {
		case "error":
			if err == nil || key != "" {
				t.Fatal("expected closed failure")
			}
		case "none":
			if err != nil || key != "" {
				t.Fatal("logout revived or read failed")
			}
		case "current":
			if err != nil || key != "synthetic-current" {
				t.Fatal("current row not selected")
			}
		case "legacy":
			if err != nil || key != "synthetic-legacy" {
				t.Fatal("pre-import fallback failed")
			}
		default:
			t.Fatal("unknown expected outcome")
		}
		p := newCurrentOpenCodeProvider(nil)
		if p.IsConfigured() != (key != "") {
			t.Fatal("public constructor disagrees with native discovery")
		}
		if key != "" && (p.apiKey != key || len(p.Strategies()) != 1 || p.Strategies()[0].Source() != "api") {
			t.Fatal("native key did not select official API")
		}
		return
	}
	_, file, _, _ := runtime.Caller(0)
	data, err := os.ReadFile(filepath.Join(filepath.Dir(file), "../../rust/symbrain-usage/tests/fixtures/opencode-v2-synthetic.json"))
	if err != nil {
		t.Fatal(err)
	}
	type scenario struct {
		ID, SQL, Outcome, Mutation string
		Imported                   *bool
		WAL                        bool
	}
	var fixture struct {
		Schema string
		Cases  []scenario
	}
	if err = json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	if len(fixture.Cases) != 45 {
		t.Fatal("case inventory changed")
	}
	for _, outcome := range []string{"current", "none", "corrupt-current", "wrong-tail-salt", "current-after-tail"} {
		expected := outcome
		if outcome != "current" && outcome != "none" {
			expected = "error"
		}
		fixture.Cases = append(fixture.Cases, scenario{ID: "wal-reuse-" + outcome, WAL: true, Outcome: expected, Mutation: "wal-reuse-" + outcome})
	}
	for _, c := range fixture.Cases {
		t.Run(c.ID, func(t *testing.T) {
			root := t.TempDir()
			dir := filepath.Join(root, "data")
			if err := os.Mkdir(dir, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(dir, "opencode.db")
			db, err := sql.Open("sqlite", path)
			if err != nil {
				t.Fatal(err)
			}
			closed := false
			defer func() {
				if !closed {
					if err := db.Close(); err != nil {
						t.Error(err)
					}
				}
			}()
			run := func(sql string) {
				t.Helper()
				if _, err := db.Exec(sql); err != nil {
					t.Fatal(err)
				}
			}
			run(fixture.Schema)
			if c.Imported == nil || *c.Imported {
				run("INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1);")
			}
			if c.WAL {
				run(`INSERT INTO credential VALUES('baseline','opencode-go','{"key":"synthetic-stale"}',1,0); PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;`)
			}
			if c.SQL != "" {
				run(c.SQL)
			} else if c.WAL {
				run("DELETE FROM credential;")
			}
			if !c.WAL {
				if err := db.Close(); err != nil {
					t.Fatal(err)
				}
				closed = true
			}
			write := func(path string, data []byte) {
				t.Helper()
				if err := os.WriteFile(path, data, 0600); err != nil {
					t.Fatal(err)
				}
			}
			write(filepath.Join(dir, "auth.json"), []byte(`{"opencode-go":{"key":"synthetic-legacy"}}`))
			must := func(err error) {
				t.Helper()
				if err != nil {
					t.Fatal(err)
				}
			}
			if strings.HasPrefix(c.Mutation, "wal-reuse-") {
				openCodeRestartWAL(t, db, path, c.Mutation)
			}
			switch c.Mutation {
			case "orphan-wal":
				must(os.Remove(path))
				write(path+"-wal", nil)
			case "dir-symlink":
				real := filepath.Join(root, "real")
				must(os.Rename(dir, real))
				must(os.Symlink(real, dir))
			case "many-entries":
				for i := 0; i < 256; i++ {
					write(filepath.Join(dir, fmt.Sprintf("extra-%d", i)), nil)
				}
			case "remove-shm":
				must(os.Remove(path + "-shm"))
			case "corrupt-db":
				write(path, []byte("not sqlite"))
			case "wal-header", "wal-frame", "wal-truncate":
				data, err := os.ReadFile(path + "-wal")
				must(err)
				if len(data) < 64 {
					t.Fatal("WAL fixture absent")
				}
				if c.Mutation == "wal-truncate" {
					data = data[:len(data)-1]
				} else if c.Mutation == "wal-header" {
					data[0] ^= 1
				} else {
					data[len(data)-1] ^= 1
				}
				write(path+"-wal", data)
			case "second-channel":
				write(filepath.Join(dir, "opencode-next.db"), []byte{})
			case "preview":
				must(os.Rename(path, filepath.Join(dir, "opencode-next.db")))
			case "db-symlink", "db-fifo":
				outside := filepath.Join(root, "outside")
				must(os.Rename(path, outside))
				if c.Mutation == "db-symlink" {
					must(os.Symlink(outside, path))
				} else {
					must(syscall.Mkfifo(path, 0600))
				}
			case "wal-symlink":
				must(os.Symlink(path, path+"-wal"))
			case "wal-fifo":
				must(syscall.Mkfifo(path+"-wal", 0600))
			case "journal":
				write(path+"-journal", nil)
			case "oversized-db":
				must(os.Truncate(path, (64<<20)+1))
			}
			snapshots := filepath.Join(root, "private-snapshots")
			must(os.Mkdir(snapshots, 0700))
			before := openCodeInputInventory(t, root)
			executable, err := os.Executable()
			must(err)
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			child := exec.CommandContext(ctx, executable, "-test.run=^TestOpenCodeV2FreshProcesses$", "-test.v")
			child.Env = []string{"HOME=" + root, "PATH=", "TMPDIR=" + snapshots, "OPENCODE_V2_CHILD_DIR=" + dir, "OPENCODE_DATA_DIR=" + dir, "OPENCODE_V2_OUTCOME=" + c.Outcome}
			output, err := child.CombinedOutput()
			if err != nil {
				t.Fatalf("bounded discovery child failed: %v\n%s", err, output)
			}
			entries, err := os.ReadDir(snapshots)
			if err != nil || len(entries) != 0 {
				t.Fatal("private snapshot cleanup failed", err)
			}
			if !reflect.DeepEqual(before, openCodeInputInventory(t, root)) {
				t.Fatal("source inventory changed")
			}
		})
	}
}

// A normal restart overwrites only a committed prefix; old physical frames remain.
func openCodeRestartWAL(t *testing.T, db *sql.DB, path, mode string) {
	t.Helper()
	run := func(query string) {
		t.Helper()
		if _, err := db.Exec(query); err != nil {
			t.Fatal(err)
		}
	}
	run("INSERT INTO credential VALUES('reuse','opencode-go','{\"key\":\"synthetic-stale\"}',1,1); CREATE TABLE padding(value BLOB); INSERT INTO padding VALUES(zeroblob(200000));")
	old, err := os.ReadFile(path + "-wal")
	if err != nil {
		t.Fatal(err)
	}
	var busy, log, checkpointed int
	if err := db.QueryRow("PRAGMA wal_checkpoint(RESTART)").Scan(&busy, &log, &checkpointed); err != nil || busy != 0 || log != checkpointed {
		t.Fatal("restart failed", err)
	}
	if mode == "wal-reuse-none" {
		run("DELETE FROM credential")
	} else {
		run("UPDATE credential SET value='{\"key\":\"synthetic-current\"}', time_updated=2")
	}
	var count int
	if err := db.QueryRow("SELECT count(*) FROM credential WHERE value='{\"key\":\"synthetic-current\"}'").Scan(&count); err != nil {
		t.Fatal(err)
	}
	if (mode == "wal-reuse-none" && count != 0) || (mode != "wal-reuse-none" && count != 1) {
		t.Fatal("SQLite current state incorrect")
	}
	data, err := os.ReadFile(path + "-wal")
	if err != nil {
		t.Fatal(err)
	}
	size := int(binary.BigEndian.Uint32(data[8:])) + 24
	end := 32
	for end+size <= len(data) && bytes.Equal(data[end+8:end+16], data[16:24]) {
		end += size
	}
	if len(data) != len(old) || end <= 32 || end >= len(data) || !bytes.Equal(data[end:], old[end:]) {
		t.Fatal("no retained obsolete tail")
	}
	t.Logf("normal WAL reuse: physical=%d current_frames=%d obsolete_frames=%d current_salts=%x old_salts=%x", len(data), (end-32)/size, (len(data)-end)/size, data[16:24], old[16:24])
	switch mode {
	case "wal-reuse-corrupt-current":
		data[end-1] ^= 1
	case "wal-reuse-wrong-tail-salt":
		data[end+8] ^= 0x40
	case "wal-reuse-current-after-tail":
		copy(data[len(data)-size+8:len(data)-size+16], data[16:24])
	default:
		return
	}
	if err := os.WriteFile(path+"-wal", data, 0600); err != nil {
		t.Fatal(err)
	}
}
