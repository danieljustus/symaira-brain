package main

import (
	"bytes"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdActivityTableAndUsageEdges(t *testing.T) {
	home := sandboxHome(t)
	writeActivityReadProfile(t, home, "activity", true)
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	dbPath := newCLIActivityTestDB(t, base)

	var searchOut, searchErr bytes.Buffer
	if code := run([]string{"activity", "search", "--profile", "activity", "--from", base.Add(-time.Minute).Format(time.RFC3339), "--to", base.Add(time.Hour).Format(time.RFC3339), "--limit", "10", "--max-tokens", "100", "--db", dbPath, "editor"}, &searchOut, &searchErr); code != exitcodes.ExitOK || !strings.Contains(searchOut.String(), "segment") {
		t.Fatalf("table search = %d, stdout=%q, stderr=%q", code, searchOut.String(), searchErr.String())
	}
	var statusOut, statusErr bytes.Buffer
	if code := run([]string{"activity", "status", "--profile", "activity", "--max-tokens", "100", "--db", dbPath}, &statusOut, &statusErr); code != exitcodes.ExitOK || !strings.Contains(statusOut.String(), "segments=1\tepisodes=1") {
		t.Fatalf("table status = %d, stdout=%q, stderr=%q", code, statusOut.String(), statusErr.String())
	}
	cases := [][]string{
		{"activity", "search", "--profile", "activity", "--from", base.Format(time.RFC3339), "--to", base.Format(time.RFC3339), "--limit", "1", "--max-tokens", "10"},
		{"activity", "get", "--profile", "activity", "missing"},
		{"activity", "status", "--profile", "activity", "unexpected", "--max-tokens", "10"},
		{"activity", "search", "--profile", "activity", "--from", base.Format(time.RFC3339), "--to", "bad", "--limit", "1", "--max-tokens", "10", "query"},
	}
	for _, args := range cases {
		var stdout, stderr bytes.Buffer
		if code := run(args, &stdout, &stderr); code != exitcodes.ExitOK && code != exitcodes.ExitNoInput {
			t.Fatalf("usage edge %v = %d, stderr=%q", args, code, stderr.String())
		}
	}
}
