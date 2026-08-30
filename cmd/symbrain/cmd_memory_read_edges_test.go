package main

import (
	"bytes"
	"errors"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

type memoryReadErrorWriter struct{}

func (memoryReadErrorWriter) Write([]byte) (int, error) {
	return 0, errors.New("memory read test writer failed")
}

func TestMemoryReadWrappersAndEmptyPaths(t *testing.T) {
	path := newMemoryReadTestDB(t)

	t.Run("list wrapper rejects invalid output format", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryList([]string{"--output", "invalid"}, &stdout, &stderr)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("exit = %v, want no input", code)
		}
		if !strings.Contains(stderr.String(), "invalid") {
			t.Fatalf("stderr = %q, want invalid format", stderr.String())
		}
	})

	t.Run("list wrapper delegates valid request", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryList([]string{"--db", path}, &stdout, &stderr)
		if code != exitcodes.ExitOK || stdout.String() != "No memories found.\n" {
			t.Fatalf("exit = %v, stdout = %q, stderr = %q", code, stdout.String(), stderr.String())
		}
	})

	t.Run("search wrapper rejects invalid output format", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemorySearch([]string{"--output", "invalid"}, &stdout, &stderr)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("exit = %v, want no input", code)
		}
		if !strings.Contains(stderr.String(), "invalid") {
			t.Fatalf("stderr = %q, want invalid format", stderr.String())
		}
	})

	t.Run("search wrapper delegates valid request", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemorySearch([]string{"--db", path, "missing"}, &stdout, &stderr)
		if code != exitcodes.ExitOK || stdout.String() != "No relevant memories found.\n" {
			t.Fatalf("exit = %v, stdout = %q, stderr = %q", code, stdout.String(), stderr.String())
		}
	})

	t.Run("list rejects unexpected positional argument", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryListWithFormat([]string{"unexpected"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("exit = %v, want no input", code)
		}
		if !strings.Contains(stderr.String(), "unexpected argument") {
			t.Fatalf("stderr = %q", stderr.String())
		}
	})

	t.Run("list rejects malformed flag", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryListWithFormat([]string{"--limit", "invalid"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("exit = %v, want no input", code)
		}
	})

	t.Run("list clamps large limit and renders empty result", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryListWithFormat([]string{"--db", path, "--limit", "1001"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		if stdout.String() != "No memories found.\n" {
			t.Fatalf("stdout = %q", stdout.String())
		}
	})

	t.Run("list reports database open failure", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemoryListWithFormat([]string{"--db", t.TempDir()}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitGeneric {
			t.Fatalf("exit = %v, want generic", code)
		}
		if !strings.Contains(stderr.String(), "open memory database") {
			t.Fatalf("stderr = %q", stderr.String())
		}
	})

	t.Run("list reports output failure", func(t *testing.T) {
		code := cmdMemoryListWithFormat([]string{"--db", path}, memoryReadErrorWriter{}, &bytes.Buffer{}, output.FormatJSON)
		if code != exitcodes.ExitGeneric {
			t.Fatalf("exit = %v, want generic", code)
		}
	})

	t.Run("search rejects missing and empty query", func(t *testing.T) {
		for name, args := range map[string][]string{
			"missing": {"--db", path},
			"empty":   {"--db", path, ""},
		} {
			t.Run(name, func(t *testing.T) {
				var stdout, stderr bytes.Buffer
				code := cmdMemorySearchWithFormat(args, &stdout, &stderr, output.FormatTable)
				if code != exitcodes.ExitNoInput {
					t.Fatalf("exit = %v, want no input", code)
				}
				if stderr.Len() == 0 {
					t.Fatal("expected validation error on stderr")
				}
			})
		}
	})

	t.Run("search rejects malformed flag", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemorySearchWithFormat([]string{"query", "--limit", "invalid"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("exit = %v, want no input", code)
		}
	})

	t.Run("search renders empty result", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemorySearchWithFormat([]string{"--db", path, "missing"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitOK {
			t.Fatalf("exit = %v, stderr = %q", code, stderr.String())
		}
		if stdout.String() != "No relevant memories found.\n" {
			t.Fatalf("stdout = %q", stdout.String())
		}
	})

	t.Run("search reports database open failure", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := cmdMemorySearchWithFormat([]string{"--db", t.TempDir(), "missing"}, &stdout, &stderr, output.FormatTable)
		if code != exitcodes.ExitGeneric {
			t.Fatalf("exit = %v, want generic", code)
		}
		if !strings.Contains(stderr.String(), "open memory database") {
			t.Fatalf("stderr = %q", stderr.String())
		}
	})

	t.Run("search reports output failure", func(t *testing.T) {
		code := cmdMemorySearchWithFormat([]string{"--db", path, "missing"}, memoryReadErrorWriter{}, &bytes.Buffer{}, output.FormatJSON)
		if code != exitcodes.ExitGeneric {
			t.Fatalf("exit = %v, want generic", code)
		}
	})
}

func TestMemoryReadTableEmptyAndNilRows(t *testing.T) {
	var list, search bytes.Buffer
	printMemoryListTable(&list, []*db.Memory{nil})
	printMemorySearchTable(&search, []db.SearchResult{{}})
	if list.String() != "ID\tSCOPE\tCREATED\tCONTENT\n" {
		t.Fatalf("list output = %q", list.String())
	}
	if search.String() != "SCORE\tID\tSCOPE\tCREATED\tCONTENT\n" {
		t.Fatalf("search output = %q", search.String())
	}
}
