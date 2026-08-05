package harness

import (
	"bytes"
	"strconv"
	"strings"
	"testing"
)

func TestUnifiedDiff_IdenticalContent_IsEmpty(t *testing.T) {
	content := []byte("{\n  \"a\": 1\n}\n")
	got := UnifiedDiff("/path/config.json", content, content)
	if got != "" {
		t.Errorf("UnifiedDiff(identical) = %q, want empty", got)
	}
}

func TestUnifiedDiff_NewFile_EveryLineAdded(t *testing.T) {
	new := []byte("{\n  \"mcpServers\": {}\n}\n")
	got := UnifiedDiff("/path/config.json", nil, new)

	if !strings.Contains(got, "--- /path/config.json") {
		t.Errorf("diff missing old header: %q", got)
	}
	if !strings.Contains(got, "+++ /path/config.json") {
		t.Errorf("diff missing new header: %q", got)
	}
	for _, line := range []string{"{", `  "mcpServers": {}`, "}"} {
		if !strings.Contains(got, "+"+line) {
			t.Errorf("diff missing added line %q: %q", line, got)
		}
	}
	if strings.Contains(got, "\n-") {
		t.Errorf("diff has a removed line for a brand-new file: %q", got)
	}
}

func TestUnifiedDiff_AddedEntry_ShowsPlusLines(t *testing.T) {
	old := []byte(`{
  "mcpServers": {}
}
`)
	new := []byte(`{
  "mcpServers": {
    "symbrain": {
      "command": "symbrain"
    }
  }
}
`)
	got := UnifiedDiff("/path/config.json", old, new)

	if !strings.Contains(got, `+    "symbrain": {`) {
		t.Errorf("diff missing added server line: %q", got)
	}
	if !strings.Contains(got, `+      "command": "symbrain"`) {
		t.Errorf("diff missing added command line: %q", got)
	}
	if !strings.Contains(got, `-  "mcpServers": {}`) {
		t.Errorf("diff missing removed line for changed mcpServers value: %q", got)
	}
}

func TestUnifiedDiff_RemovedEntry_ShowsMinusLines(t *testing.T) {
	old := []byte(`{
  "mcpServers": {
    "symbrain": {
      "command": "symbrain"
    }
  }
}
`)
	new := []byte(`{
  "mcpServers": {}
}
`)
	got := UnifiedDiff("/path/config.json", old, new)

	if !strings.Contains(got, `-    "symbrain": {`) {
		t.Errorf("diff missing removed server line: %q", got)
	}
	if !strings.Contains(got, `+  "mcpServers": {}`) {
		t.Errorf("diff missing added collapsed line: %q", got)
	}
}

func TestUnifiedDiff_HasHunkHeader(t *testing.T) {
	old := []byte("a\nb\nc\n")
	new := []byte("a\nX\nc\n")
	got := UnifiedDiff("f", old, new)
	if !strings.Contains(got, "@@ ") {
		t.Errorf("diff missing hunk header: %q", got)
	}
}

// TestUnifiedDiff_SmallFile_ByteIdentical pins the exact output for a small
// file so any future change to the LCS path (including the size cap) is
// caught as a byte-level behavior change.
func TestUnifiedDiff_SmallFile_ByteIdentical(t *testing.T) {
	old := []byte("a\nb\nc\n")
	new := []byte("a\nX\nc\n")
	want := "--- f\n+++ f\n@@ -1,3 +1,3 @@\n a\n-b\n+X\n c\n"
	if got := UnifiedDiff("f", old, new); got != want {
		t.Errorf("UnifiedDiff small-file output changed:\ngot:  %q\nwant: %q", got, want)
	}
}

// TestUnifiedDiff_TooLarge_ReturnsNotice exercises the fallback boundary
// just above maxDiffLines: the O(n·m) LCS table must be skipped in favor of
// an explicit notice that still carries the unified-diff file headers.
func TestUnifiedDiff_TooLarge_ReturnsNotice(t *testing.T) {
	t.Run("old side above cap", func(t *testing.T) {
		old := bytes.Repeat([]byte("old line\n"), maxDiffLines+1)
		got := UnifiedDiff("f", old, []byte("new line\n"))
		checkTooLargeNotice(t, got, maxDiffLines+1, 1)
	})
	t.Run("new side above cap", func(t *testing.T) {
		new := bytes.Repeat([]byte("new line\n"), maxDiffLines+1)
		got := UnifiedDiff("f", []byte("old line\n"), new)
		checkTooLargeNotice(t, got, 1, maxDiffLines+1)
	})
	t.Run("brand-new file above cap", func(t *testing.T) {
		new := bytes.Repeat([]byte("new line\n"), maxDiffLines+1)
		got := UnifiedDiff("f", nil, new)
		checkTooLargeNotice(t, got, 0, maxDiffLines+1)
	})
}

func checkTooLargeNotice(t *testing.T, got string, oldCount, newCount int) {
	t.Helper()
	if !strings.HasPrefix(got, "--- f\n+++ f\n") {
		t.Fatalf("notice missing unified-diff file headers: %q", truncate(got))
	}
	if !strings.Contains(got, "file too large to diff") {
		t.Errorf("notice missing explanation: %q", truncate(got))
	}
	if !strings.Contains(got, "max "+strconv.Itoa(maxDiffLines)) {
		t.Errorf("notice missing cap value: %q", truncate(got))
	}
	// The notice must not contain per-line diff content.
	for _, marker := range []string{"\n-old line", "\n+new line"} {
		if strings.Contains(got, marker) {
			t.Errorf("notice unexpectedly contains per-line content %q: %q", marker, truncate(got))
		}
	}
}

// TestUnifiedDiff_AtCap_StillRunsFullDiff exercises the boundary just below
// the cap: exactly maxDiffLines lines on one side must still take the full
// LCS path and produce a real diff, not the notice. The other side is kept
// tiny so the test itself stays cheap.
func TestUnifiedDiff_AtCap_StillRunsFullDiff(t *testing.T) {
	old := bytes.Repeat([]byte("l\n"), maxDiffLines)
	got := UnifiedDiff("f", old, []byte("l\n"))

	if strings.Contains(got, "file too large to diff") {
		t.Fatalf("expected full diff at cap boundary, got notice: %q", truncate(got))
	}
	if !strings.HasPrefix(got, "--- f\n+++ f\n@@ -1,10000 +1,1 @@\n") {
		t.Errorf("unexpected hunk header at cap boundary: %q", truncate(got))
	}
	if !strings.Contains(got, "-l\n") || !strings.Contains(got, " l\n") {
		t.Errorf("expected deleted and context lines at cap boundary: %q", truncate(got))
	}
}

func truncate(s string) string {
	const max = 200
	if len(s) > max {
		return s[:max] + "..."
	}
	return s
}
