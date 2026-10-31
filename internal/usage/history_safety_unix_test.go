//go:build unix

package usage

import (
	"context"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

type historyTransport struct{}

func (historyTransport) RoundTrip(*http.Request) (*http.Response, error) {
	return &http.Response{StatusCode: 412, Body: io.NopCloser(strings.NewReader(`{"error":"No personal team"}`)), Header: make(http.Header)}, nil
}

var httpClientHistory = http.Client{Transport: historyTransport{}}

func TestGrokHistorySafetyAndBounds(t *testing.T) {
	cases := loadHistoryCases(t)
	for _, kind := range []string{"file-link", "dir-link", "home-link", "fifo", "oversize", "depth", "files", "cancel", "dotdot", "relative", "entries", "events", "line"} {
		t.Run(kind, func(t *testing.T) {
			home := historyFixture(t, map[string]string{"p/updates.jsonl": cases.Cases[1].Files["p/updates.jsonl"]})
			path := filepath.Join(home, "sessions", "p", "updates.jsonl")
			ctx := context.Background()
			switch kind {
			case "file-link":
				if err := os.Remove(path); err != nil {
					t.Fatal(err)
				}
				if err := os.Symlink(filepath.Join(home, "outside"), path); err != nil {
					t.Fatal(err)
				}
			case "dir-link":
				if err := os.Symlink(home, filepath.Join(home, "sessions", "alias")); err != nil {
					t.Fatal(err)
				}
			case "home-link":
				link := filepath.Join(t.TempDir(), "alias")
				if err := os.Symlink(home, link); err != nil {
					t.Fatal(err)
				}
				home = link
			case "fifo":
				if err := os.Remove(path); err != nil {
					t.Fatal(err)
				}
				if err := unix.Mkfifo(path, 0600); err != nil {
					t.Fatal(err)
				}
			case "oversize":
				if err := os.WriteFile(path, make([]byte, (4<<20)+1), 0600); err != nil {
					t.Fatal(err)
				}
			case "depth":
				if err := os.MkdirAll(filepath.Join(home, "sessions", strings.Repeat("d/", 14)), 0700); err != nil {
					t.Fatal(err)
				}
			case "files":
				for i := 0; i < 129; i++ {
					dir := filepath.Join(home, "sessions", time.Unix(int64(i), 0).Format("150405"))
					if err := os.MkdirAll(dir, 0700); err != nil {
						t.Fatal(err)
					}
					if err := os.WriteFile(filepath.Join(dir, "updates.jsonl"), nil, 0600); err != nil {
						t.Fatal(err)
					}
				}
			case "cancel":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			case "dotdot":
				home += "/../" + filepath.Base(home)
			case "relative":
				home = "relative"
			case "entries":
				for i := 0; i < 4097; i++ {
					if err := os.WriteFile(filepath.Join(home, "sessions", time.Unix(int64(i), 0).Format("150405")), nil, 0600); err != nil {
						t.Fatal(err)
					}
				}
			case "events":
				if err := os.WriteFile(path, []byte(strings.Repeat(cases.Cases[1].Files["p/updates.jsonl"], 10001)), 0600); err != nil {
					t.Fatal(err)
				}
			case "line":
				if err := os.WriteFile(path, []byte(strings.Repeat(" ", 256<<10)+"1\n"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			start := time.Now()
			h := ScanGrokHistory(ctx, home, time.Now())
			if h.Status != "unavailable" || len(h.Daily) != 0 {
				t.Fatal(h)
			}
			if time.Since(start) > 3*time.Second {
				t.Fatal("unbounded read")
			}
		})
	}
}
