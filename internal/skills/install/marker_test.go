package install

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// The fixture triad pins the read rule: a missing schema_version is
// version 1, the current writer emits 1, and a one-version-ahead marker
// parses as 2 (and is refused by every writer).
func TestMarkerSchemaVersionFixtures(t *testing.T) {
	cases := []struct {
		name string
		file string
		want int
	}{
		{"minimal marker without schema_version reads as 1", "marker-minimal.json", 1},
		{"full marker carries schema_version 1", "marker-full.json", 1},
		{"one-version-ahead marker reads as 2", "marker-ahead.json", 2},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			data, err := os.ReadFile(filepath.Join("testdata", c.file))
			if err != nil {
				t.Fatal(err)
			}
			got, err := markerSchemaVersion(data)
			if err != nil {
				t.Fatalf("markerSchemaVersion: %v", err)
			}
			if got != c.want {
				t.Fatalf("schema_version: want %d, got %d", c.want, got)
			}
		})
	}
}

func TestCheckMarkerWritableRefusesNewerVersion(t *testing.T) {
	marker := filepath.Join(t.TempDir(), markerFile)

	if err := checkMarkerWritable(marker); err != nil {
		t.Fatalf("absent marker must be writable: %v", err)
	}
	if err := os.WriteFile(marker, []byte(`{"schema_version": 1}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := checkMarkerWritable(marker); err != nil {
		t.Fatalf("current-version marker must be writable: %v", err)
	}
	if err := os.WriteFile(marker, []byte(`{"schema_version": 2}`), 0o644); err != nil {
		t.Fatal(err)
	}
	err := checkMarkerWritable(marker)
	if err == nil {
		t.Fatal("expected refusal for a newer schema_version")
	}
	if !strings.Contains(err.Error(), "schema_version 2") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestInstallRefusesNewerMarkerInRenderCache(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: newer\ndescription: test\n---\n")
	// A newer client (or symskills) wrote the render-cache marker; this build
	// must refuse to overwrite it with its own version-1 marker.
	writeFile(t, filepath.Join(rendered, markerFile), `{"schema_version": 2, "managed_by": "symskills"}`)

	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "newer",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err == nil {
		t.Fatal("expected refusal when the render-cache marker is one version ahead")
	}
}

func TestInstallRefusesNewerMarkerAtDest(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: newer-dest\ndescription: test\n---\n")
	dest := filepath.Join(home, ".config", "opencode", "skills", "newer-dest")
	writeFile(t, filepath.Join(dest, "SKILL.md"), "managed by a newer client")
	writeFile(t, filepath.Join(dest, markerFile), `{"schema_version": 2, "managed_by": "symskills"}`)

	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "newer-dest",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err == nil {
		t.Fatal("expected refusal when the destination marker is one version ahead")
	}
	data, err := os.ReadFile(filepath.Join(dest, "SKILL.md"))
	if err != nil || string(data) != "managed by a newer client" {
		t.Fatalf("newer-managed install must be left untouched, got %q err=%v", string(data), err)
	}
}
