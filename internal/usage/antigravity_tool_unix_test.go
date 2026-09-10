//go:build unix

package usage

import (
	"os"
	"path/filepath"
	"testing"
)

func TestResolveProbeToolUsesAbsolutePATHEntry(t *testing.T) {
	dir := t.TempDir()
	tool := filepath.Join(dir, "ps")
	if err := os.WriteFile(tool, []byte("#!/bin/sh\nexit 0\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", dir)
	got, ok := resolveProbeTool("ps")
	if !ok {
		t.Fatal("fixed probe tool was not resolved from PATH")
	}
	if got != tool || !filepath.IsAbs(got) {
		t.Fatalf("resolved path = %q, want absolute %q", got, tool)
	}
	if _, ok := resolveProbeTool("not-allowed"); ok {
		t.Fatal("missing probe tool unexpectedly resolved")
	}
}
