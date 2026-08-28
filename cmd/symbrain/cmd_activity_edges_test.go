package main

import (
	"bytes"
	"io"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

type failingActivityWriter struct{}

func (failingActivityWriter) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func TestCmdActivityDirectAndErrorPaths(t *testing.T) {
	home := sandboxHome(t)
	writeActivityReadProfile(t, home, "activity", true)
	cases := []struct {
		name string
		args []string
		want string
	}{
		{name: "empty command", args: nil, want: "bounded, profile-gated activity reads"},
		{name: "equals profile", args: []string{"status", "--profile=activity", "--max-tokens", "10"}, want: "segments=0"},
		{name: "unknown profile", args: []string{"status", "--profile=missing", "--max-tokens", "10"}, want: "explicitly expose activity read tools"},
		{name: "missing query", args: []string{"search", "--profile", "activity", "--from", "2026-08-28T12:00:00Z", "--to", "2026-08-28T13:00:00Z", "--limit", "1", "--max-tokens", "10"}, want: "usage: symbrain activity search"},
	}
	for _, tt := range cases {
		t.Run(tt.name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := cmdActivity(tt.args, &stdout, &stderr)
			if tt.name == "equals profile" {
				if code != exitcodes.ExitOK || !bytes.Contains(stdout.Bytes(), []byte(tt.want)) {
					t.Fatalf("equals profile = %d, stdout=%q stderr=%q", code, stdout.String(), stderr.String())
				}
				return
			}
			if tt.name == "empty command" {
				if code != exitcodes.ExitNoInput || !bytes.Contains(stdout.Bytes(), []byte(tt.want)) {
					t.Fatalf("empty command = %d, stdout=%q stderr=%q", code, stdout.String(), stderr.String())
				}
				return
			}
			if code == exitcodes.ExitOK || !bytes.Contains(stderr.Bytes(), []byte(tt.want)) {
				t.Fatalf("code=%d stdout=%q stderr=%q, want %q", code, stdout.String(), stderr.String(), tt.want)
			}
		})
	}

	var stderr bytes.Buffer
	if _, _, _, _, _, _, _, ok := activityFlags("activity", []string{"--limit", "not-an-int"}, &stderr); ok {
		t.Fatal("invalid integer flag unexpectedly accepted")
	}

	for _, name := range []string{"search", "get", "status"} {
		var stdout, errout bytes.Buffer
		args := []string{name, "--profile", "activity", "--max-tokens", "10", "--db", t.TempDir()}
		if name == "search" {
			args = append(args, "--from", "2026-08-28T12:00:00Z", "--to", "2026-08-28T13:00:00Z", "--limit", "1", "query")
		}
		if name == "get" {
			args = append(args, "missing")
		}
		code := cmdActivityWithFormat(args, &stdout, &errout, output.FormatTable)
		if code != exitcodes.ExitGeneric || !bytes.Contains(errout.Bytes(), []byte("open database")) {
			t.Fatalf("%s database error = %d, stdout=%q stderr=%q", name, code, stdout.String(), errout.String())
		}
	}
}

func TestRenderCLIActivityReportsWriterErrors(t *testing.T) {
	if code := renderCLIActivity(failingActivityWriter{}, output.FormatJSON, struct{}{}, func(io.Writer) {}); code != exitcodes.ExitGeneric {
		t.Fatalf("render error code = %d", code)
	}
}
