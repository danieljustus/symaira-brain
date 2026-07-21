package harness

import (
	"os"
	"path/filepath"
	"testing"
)

// TestGolden_InsertSymbrainEntry proves that, for every registered harness,
// inserting the symbrain MCP entry into an existing config preserves all
// unrelated content: parsing testdata/golden/<harness>/before.<ext>,
// setting the symbrain server entry, and re-marshaling must reproduce
// testdata/golden/<harness>/after.<ext> byte-for-byte.
func TestGolden_InsertSymbrainEntry(t *testing.T) {
	profiles := map[Name]string{
		Claude:        "personal",
		ClaudeDesktop: "restricted",
		Cursor:        "personal",
		Opencode:      "default",
		Codex:         "personal",
		Gemini:        "default",
	}

	for _, h := range All {
		t.Run(string(h.Name), func(t *testing.T) {
			ext := "json"
			if h.Format == FormatTOML {
				ext = "toml"
			}
			dir := filepath.Join("testdata", "golden", string(h.Name))

			before, err := os.ReadFile(filepath.Join(dir, "before."+ext))
			if err != nil {
				t.Fatalf("read before fixture: %v", err)
			}
			want, err := os.ReadFile(filepath.Join(dir, "after."+ext))
			if err != nil {
				t.Fatalf("read after fixture: %v", err)
			}

			doc, err := Parse(h, before)
			if err != nil {
				t.Fatalf("Parse(before): %v", err)
			}

			profile, ok := profiles[h.Name]
			if !ok {
				t.Fatalf("no profile fixture wired up for harness %q", h.Name)
			}
			doc.SetServer(ServerName, NewEntry(profile))

			got, err := doc.Marshal()
			if err != nil {
				t.Fatalf("Marshal(after): %v", err)
			}

			if string(got) != string(want) {
				t.Errorf("Marshal() after SetServer mismatch for %s\n--- got ---\n%s\n--- want ---\n%s", h.Name, got, want)
			}
		})
	}
}

// TestGolden_BeforeFixturesAreCanonical asserts every before.<ext> fixture
// is already in the package's canonical (idempotent) form: Parse followed
// by Marshal with no edits reproduces it exactly. Golden comparisons above
// only mean something if this holds — otherwise a golden mismatch could be
// masking an unrelated formatting drift instead of a real content bug.
func TestGolden_BeforeFixturesAreCanonical(t *testing.T) {
	for _, h := range All {
		t.Run(string(h.Name), func(t *testing.T) {
			ext := "json"
			if h.Format == FormatTOML {
				ext = "toml"
			}
			dir := filepath.Join("testdata", "golden", string(h.Name))

			before, err := os.ReadFile(filepath.Join(dir, "before."+ext))
			if err != nil {
				t.Fatalf("read before fixture: %v", err)
			}

			doc, err := Parse(h, before)
			if err != nil {
				t.Fatalf("Parse(before): %v", err)
			}
			got, err := doc.Marshal()
			if err != nil {
				t.Fatalf("Marshal(before): %v", err)
			}

			if string(got) != string(before) {
				t.Errorf("before fixture for %s is not idempotent under Parse+Marshal\n--- got ---\n%s\n--- want (original) ---\n%s", h.Name, got, before)
			}
		})
	}
}

// TestGolden_RemoveServerRestoresBefore proves the JSON/TOML edit is
// reversible: parsing after.<ext>, removing the symbrain entry, and
// re-marshaling reproduces before.<ext> byte-for-byte. This is the same
// invariant cmd_uninstall.go's round-trip test relies on.
func TestGolden_RemoveServerRestoresBefore(t *testing.T) {
	for _, h := range All {
		t.Run(string(h.Name), func(t *testing.T) {
			ext := "json"
			if h.Format == FormatTOML {
				ext = "toml"
			}
			dir := filepath.Join("testdata", "golden", string(h.Name))

			after, err := os.ReadFile(filepath.Join(dir, "after."+ext))
			if err != nil {
				t.Fatalf("read after fixture: %v", err)
			}
			want, err := os.ReadFile(filepath.Join(dir, "before."+ext))
			if err != nil {
				t.Fatalf("read before fixture: %v", err)
			}

			doc, err := Parse(h, after)
			if err != nil {
				t.Fatalf("Parse(after): %v", err)
			}
			if removed := doc.RemoveServer(ServerName); !removed {
				t.Fatalf("RemoveServer(%q) = false, want true", ServerName)
			}

			got, err := doc.Marshal()
			if err != nil {
				t.Fatalf("Marshal(after RemoveServer): %v", err)
			}
			if string(got) != string(want) {
				t.Errorf("Marshal() after RemoveServer mismatch for %s\n--- got ---\n%s\n--- want ---\n%s", h.Name, got, want)
			}
		})
	}
}
