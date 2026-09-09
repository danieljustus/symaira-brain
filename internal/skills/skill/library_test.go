package skill

import (
	"os"
	"path/filepath"
	"testing"
)

func TestImportSkillCopiesExistingSkillIntoLibrary(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), `---
name: import-me
description: Import this existing OpenCode style skill.
---

Body.
`)
	writeFile(t, filepath.Join(src, "references", "details.md"), "Details\n")

	dst := filepath.Join(t.TempDir(), "library")
	imported, err := ImportSkill(src, dst)
	if err != nil {
		t.Fatalf("ImportSkill: %v", err)
	}

	if imported.Name != "import-me" {
		t.Fatalf("imported name: %q", imported.Name)
	}
	if _, err := os.Stat(filepath.Join(dst, "import-me", "SKILL.md")); err != nil {
		t.Fatalf("imported SKILL.md missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "import-me", "references", "details.md")); err != nil {
		t.Fatalf("imported reference missing: %v", err)
	}
}

func TestValidateWithOverlays(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), `---
name: overlay-test
description: Testing safeRelativeFile
---
Body.
`)

	// 1. Missing reference file
	writeFile(t, filepath.Join(dir, "symskills.toml"), `[skill]
name = "overlay-test"

[targets.opencode]
enabled = true
prepend = "prep.md"
`)

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatal(err)
	}

	issues := Validate(bundle)
	if !HasIssue(issues, "overlay_reference_missing") {
		t.Fatalf("expected overlay_reference_missing issue, got: %#v", issues)
	}

	// 2. Absolute path reference
	writeFile(t, filepath.Join(dir, "symskills.toml"), `[skill]
name = "overlay-test"

[targets.opencode]
enabled = true
prepend = "/abs/path.md"
`)
	bundle, _ = LoadBundle(dir)
	issues = Validate(bundle)
	if !HasIssue(issues, "overlay_reference_missing") {
		t.Fatalf("expected overlay_reference_missing issue on absolute path, got: %#v", issues)
	}

	// 3. Escaping path reference
	writeFile(t, filepath.Join(dir, "symskills.toml"), `[skill]
name = "overlay-test"

[targets.opencode]
enabled = true
prepend = "../escape.md"
`)
	bundle, _ = LoadBundle(dir)
	issues = Validate(bundle)
	if !HasIssue(issues, "overlay_reference_missing") {
		t.Fatalf("expected overlay_reference_missing issue on escaping path, got: %#v", issues)
	}

	// 4. Valid reference
	writeFile(t, filepath.Join(dir, "prep.md"), "prepend content")
	writeFile(t, filepath.Join(dir, "symskills.toml"), `[skill]
name = "overlay-test"

[targets.opencode]
enabled = true
prepend = "prep.md"
`)
	bundle, _ = LoadBundle(dir)
	issues = Validate(bundle)
	if HasIssue(issues, "overlay_reference_missing") {
		t.Fatalf("unexpected overlay_reference_missing issue for valid prepend: %#v", issues)
	}
}

func TestImportSkillsBatchImportsMultipleSubdirectories(t *testing.T) {
	src := t.TempDir()

	// Create two valid skill subdirectories
	skill1 := filepath.Join(src, "skill-alpha")
	writeFile(t, filepath.Join(skill1, "SKILL.md"), `---
name: skill-alpha
description: First test skill.
---
Body alpha.
`)
	skill2 := filepath.Join(src, "skill-beta")
	writeFile(t, filepath.Join(skill2, "SKILL.md"), `---
name: skill-beta
description: Second test skill.
---
Body beta.
`)

	// Create a non-skill directory (no SKILL.md)
	noSkill := filepath.Join(src, "not-a-skill")
	if err := os.MkdirAll(noSkill, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a broken skill directory (SKILL.md without frontmatter)
	broken := filepath.Join(src, "broken-skill")
	writeFile(t, filepath.Join(broken, "SKILL.md"), "No frontmatter here.\n")

	dst := filepath.Join(t.TempDir(), "library")
	results := ImportSkills(src, dst)

	if len(results) < 2 {
		t.Fatalf("expected at least 2 results, got %d: %#v", len(results), results)
	}

	imported := make(map[string]BatchImportResult)
	var failed int
	for _, r := range results {
		switch r.Status {
		case BatchImported:
			imported[r.Name] = r
		case BatchFailed:
			failed++
		}
	}

	if _, ok := imported["skill-alpha"]; !ok {
		t.Errorf("skill-alpha was not imported: results=%#v", results)
	}
	if _, ok := imported["skill-beta"]; !ok {
		t.Errorf("skill-beta was not imported: results=%#v", results)
	}
	if failed < 1 {
		t.Errorf("expected at least 1 failed (broken-skill), got %d", failed)
	}

	// Verify imported skills were actually written
	for name, r := range imported {
		if _, err := os.Stat(filepath.Join(dst, name, "SKILL.md")); err != nil {
			t.Errorf("imported skill %q SKILL.md missing at %s: %v", name, r.Path, err)
		}
	}
}

func TestImportSkillsFallsBackForSingleSkillDir(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), `---
name: solo-skill
description: A single skill directory.
---
Body.
`)
	dst := filepath.Join(t.TempDir(), "library")
	results := ImportSkills(src, dst)

	if len(results) != 1 {
		t.Fatalf("expected 1 result for single-skill dir, got %d: %#v", len(results), results)
	}
	if results[0].Status != BatchImported {
		t.Fatalf("expected imported, got %s: %#v", results[0].Status, results)
	}
	if results[0].Name != "solo-skill" {
		t.Fatalf("expected name solo-skill, got %q", results[0].Name)
	}
	if _, err := os.Stat(filepath.Join(dst, "solo-skill", "SKILL.md")); err != nil {
		t.Fatalf("imported SKILL.md missing: %v", err)
	}
}

func TestImportSkillsDuplicateIsSkipped(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "dup-skill", "SKILL.md"), `---
name: dup-skill
description: Test duplicate.
---
Body.
`)

	dst := filepath.Join(t.TempDir(), "library")
	// First import
	r1 := ImportSkills(src, dst)
	if len(r1) != 1 || r1[0].Status != BatchImported {
		t.Fatalf("first import should succeed: %#v", r1)
	}

	// Second import (same skill already exists)
	r2 := ImportSkills(src, dst)
	if len(r2) != 1 || r2[0].Status != BatchFailed {
		t.Fatalf("second import should fail (duplicate): %#v", r2)
	}
	if r2[0].Error == "" {
		t.Fatal("expected error message for duplicate")
	}
}

func TestListLibrary(t *testing.T) {
	// 1. Nonexistent library directory
	bundles, issues := ListLibrary("/nonexistent/library")
	if bundles != nil || issues != nil {
		t.Fatalf("expected nil list and issues for nonexistent library, got bundles=%v, issues=%v", bundles, issues)
	}

	// 2. Healthy library directory
	lib := t.TempDir()
	skill1 := filepath.Join(lib, "skill-one")
	writeFile(t, filepath.Join(skill1, "SKILL.md"), "---\nname: skill-one\ndescription: Test\n---\nBody\n")

	// Add a non-directory entry to test it's skipped
	if err := os.WriteFile(filepath.Join(lib, "regular-file.txt"), []byte("data"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Add a broken skill directory
	skill2 := filepath.Join(lib, "skill-two")
	if err := os.MkdirAll(skill2, 0o755); err != nil {
		t.Fatal(err)
	}

	bundles, issues = ListLibrary(lib)
	if len(bundles) != 1 || bundles[0].Frontmatter.Name != "skill-one" {
		t.Errorf("expected 1 bundle skill-one, got bundles: %#v", bundles)
	}
	if len(issues) != 1 || issues[0].Code != "skill_load" {
		t.Errorf("expected 1 skill_load issue, got: %#v", issues)
	}
}
