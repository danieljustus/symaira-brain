//go:build unix

package usage

import (
	"context"
	"fmt"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestClaudeHistoryNativeBounds(t *testing.T) {
	for _, name := range []string{"file-size", "line-size", "entry-count", "file-count", "line-count", "event-count", "depth", "symlink-file", "symlink-ancestor", "fifo", "cancel", "relative", "dotdot"} {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			project := filepath.Join(dir, "projects")
			if err := os.Mkdir(project, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(project, "session.jsonl")
			ctx := context.Background()
			scanDir := dir
			write := func(path, data string) {
				t.Helper()
				if err := os.WriteFile(path, []byte(data), 0600); err != nil {
					t.Fatal(err)
				}
			}
			switch name {
			case "file-size":
				f, err := os.Create(path)
				if err != nil {
					t.Fatal(err)
				}
				if err = f.Truncate((4 << 20) + 1); err != nil {
					t.Fatal(err)
				}
				if err = f.Close(); err != nil {
					t.Fatal(err)
				}
			case "line-size":
				write(path, strings.Repeat(" ", (256<<10)+1)+"x\n")
			case "entry-count":
				for i := 0; i < 4097; i++ {
					write(filepath.Join(project, fmt.Sprint(i)), "")
				}
			case "file-count":
				for i := 0; i < 129; i++ {
					write(filepath.Join(project, fmt.Sprintf("%03d.jsonl", i)), "")
				}
			case "line-count":
				write(path, strings.Repeat("{}\n", 20001))
			case "event-count":
				line := `{"type":"assistant","timestamp":"2026-10-20T10:00:00Z","requestId":"r","message":{"id":"m","role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}}` + "\n"
				write(path, strings.Repeat(line, 10001))
			case "depth":
				p := filepath.Join(project, strings.Repeat("sub/", 13))
				if err := os.MkdirAll(p, 0700); err != nil {
					t.Fatal(err)
				}
			case "symlink-file":
				if err := os.Symlink("absent", path); err != nil {
					t.Fatal(err)
				}
			case "symlink-ancestor":
				link := filepath.Join(t.TempDir(), "link")
				if err := os.Symlink(dir, link); err != nil {
					t.Fatal(err)
				}
				scanDir = link
			case "fifo":
				if err := unix.Mkfifo(path, 0600); err != nil {
					t.Fatal(err)
				}
			case "cancel":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			case "relative":
				scanDir = "relative"
			case "dotdot":
				scanDir = dir + "/../" + filepath.Base(dir)
			}
			h := ScanClaudeHistory(ctx, scanDir, time.Now())
			if h.Status != "unavailable" || len(h.Daily) != 0 || len(h.Periods) != 0 {
				t.Fatalf("%+v", h)
			}
		})
	}
}
