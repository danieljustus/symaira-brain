package skill

import (
	"path/filepath"
	"strings"
	"testing"
)

func TestFrontmatterAcceptsStringListsForStringFields(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), `---
name: list-frontmatter
description: [First, Second]
author: [A, B]
version: [1, 2]
license: [Apache-2.0, MIT]
compatibility: [macOS, Linux]
---
Body.
`)

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	checks := map[string]string{
		"description":   "First, Second",
		"author":        "A, B",
		"version":       "1, 2",
		"license":       "Apache-2.0, MIT",
		"compatibility": "macOS, Linux",
	}
	got := map[string]string{
		"description":   bundle.Frontmatter.Description,
		"author":        bundle.Frontmatter.Author,
		"version":       bundle.Frontmatter.Version,
		"license":       bundle.Frontmatter.License,
		"compatibility": bundle.Frontmatter.Compatibility,
	}
	for field, want := range checks {
		if got[field] != want {
			t.Errorf("%s: want %q, got %q", field, want, got[field])
		}
	}
}

func TestFrontmatterRejectsInvalidStringFieldWithFieldName(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), `---
name: invalid-frontmatter
description:
  nested: value
---
Body.
`)

	_, err := LoadBundle(dir)
	if err == nil {
		t.Fatal("expected invalid description to fail parsing")
	}
	if !strings.Contains(err.Error(), `frontmatter field "description"`) {
		t.Fatalf("expected field-specific error, got %v", err)
	}
}
