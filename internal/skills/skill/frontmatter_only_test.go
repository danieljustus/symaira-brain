package skill

import (
	"path/filepath"
	"testing"
)

func TestListLibraryFrontmatterOnlyMalformedCases(t *testing.T) {
	lib := t.TempDir()

	writeFile(t, filepath.Join(lib, "no-frontmatter", "SKILL.md"), "Just a body, no frontmatter fence\n")
	writeFile(t, filepath.Join(lib, "unclosed-frontmatter", "SKILL.md"), "---\nname: unclosed\ndescription: test\n")
	writeFile(t, filepath.Join(lib, "bad-yaml", "SKILL.md"), "---\nname: [this is not valid yaml\n---\nBody\n")
	writeFile(t, filepath.Join(lib, "healthy", "SKILL.md"), "---\nname: healthy\ndescription: Test\nmetadata:\n  type: user\n---\nBody\n")

	bundles, issues := ListLibrary(lib)

	byName := map[string]*Bundle{}
	for _, b := range bundles {
		byName[b.Frontmatter.Name] = b
	}
	if _, ok := byName["healthy"]; !ok {
		t.Fatalf("expected healthy skill to load, got bundles: %#v", bundles)
	}
	if byName["healthy"].Frontmatter.Metadata["type"] != "user" {
		t.Errorf("expected metadata type=user, got %#v", byName["healthy"].Frontmatter.Metadata)
	}

	if len(issues) != 3 {
		t.Fatalf("expected 3 skill_load issues for malformed SKILL.md files, got %d: %#v", len(issues), issues)
	}
	for _, iss := range issues {
		if iss.Code != "skill_load" {
			t.Errorf("expected skill_load issue code, got %q", iss.Code)
		}
	}
}

func TestListLibraryFrontmatterOnlyDefaultsEmptyMetadata(t *testing.T) {
	lib := t.TempDir()
	writeFile(t, filepath.Join(lib, "no-metadata", "SKILL.md"), "---\nname: no-metadata\ndescription: Test\n---\nBody\n")

	bundles, issues := ListLibrary(lib)
	if len(issues) != 0 {
		t.Fatalf("unexpected issues: %#v", issues)
	}
	if len(bundles) != 1 {
		t.Fatalf("expected 1 bundle, got %d", len(bundles))
	}
	if bundles[0].Frontmatter.Metadata == nil {
		t.Error("expected non-nil metadata map even without an explicit metadata section")
	}
}
