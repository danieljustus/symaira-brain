package render

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func TestCachedStagingRenderInvalidatesMetadataTemplate(t *testing.T) {
	before := len(Targets)
	t.Cleanup(func() { Targets = Targets[:before] })

	template := filepath.Join(t.TempDir(), "metadata.txt")
	if err := os.WriteFile(template, []byte("version one\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := RegisterCustomTargets([]CustomTargetSpec{
		{
			Name:             "metadata-cache",
			SkillRootUser:    "/tmp/metadata-cache/skills",
			MetadataFile:     "meta/config.txt",
			MetadataTemplate: template,
		},
	}); err != nil {
		t.Fatal(err)
	}

	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "SKILL.md"), []byte("---\nname: cache-skill\ndescription: test\n---\nBody.\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	cache := filepath.Join(t.TempDir(), "cache")
	first, cleanup, err := CachedStagingRender(bundle, []Target{"metadata-cache"}, cache)
	if err != nil {
		t.Fatal(err)
	}
	cleanup()
	firstData, err := os.ReadFile(filepath.Join(first[0].Path, "meta", "config.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(firstData) != "version one\n" {
		t.Fatalf("first metadata = %q", firstData)
	}

	if err := os.WriteFile(template, []byte("version two\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	second, cleanup, err := CachedStagingRender(bundle, []Target{"metadata-cache"}, cache)
	if err != nil {
		t.Fatal(err)
	}
	cleanup()
	secondData, err := os.ReadFile(filepath.Join(second[0].Path, "meta", "config.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(secondData) != "version two\n" {
		t.Fatalf("cached metadata was stale: %q", secondData)
	}
}
