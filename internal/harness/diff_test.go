package harness

import (
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
