package skill

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLoadBundleParsesFrontmatterAndManifest(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), `---
name: sample-skill
description: Use when testing Symaira skill parsing.
category: Testing
license: Apache-2.0
metadata:
  audience: maintainers
---

# Sample Skill

Follow the workflow.
`)
	writeFile(t, filepath.Join(dir, "symskills.toml"), `[skill]
name = "sample-skill"
version = "1.2.3"
source = "https://example.test/sample-skill"

[targets.opencode]
enabled = true
alias = "sample-opencode"

[targets.claude]
enabled = false
`)

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}

	if bundle.Frontmatter.Name != "sample-skill" {
		t.Fatalf("name: want sample-skill, got %q", bundle.Frontmatter.Name)
	}
	if bundle.Frontmatter.Category != "Testing" {
		t.Fatalf("category: want Testing, got %q", bundle.Frontmatter.Category)
	}
	if bundle.Frontmatter.Metadata["audience"] != "maintainers" {
		t.Fatalf("metadata audience not parsed: %#v", bundle.Frontmatter.Metadata)
	}
	if bundle.Manifest.Skill.Version != "1.2.3" {
		t.Fatalf("manifest version: want 1.2.3, got %q", bundle.Manifest.Skill.Version)
	}
	if !bundle.Manifest.Targets["opencode"].Enabled {
		t.Fatal("opencode target should be enabled")
	}
	if bundle.Manifest.Targets["opencode"].Alias != "sample-opencode" {
		t.Fatal("opencode alias should be parsed")
	}
	if bundle.Manifest.Targets["claude"].Enabled {
		t.Fatal("claude target should be disabled")
	}
	if bundle.Body != "# Sample Skill\n\nFollow the workflow.\n" {
		t.Fatalf("body mismatch: %q", bundle.Body)
	}
}

func TestValidateRejectsInvalidNamesAndMissingDescription(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), `---
name: Bad_Name
description: ""
---

Body.
`)

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}

	issues := Validate(bundle)
	if len(issues) == 0 {
		t.Fatal("expected validation issues")
	}
	if !HasIssue(issues, "name_format") {
		t.Fatalf("expected name_format issue, got %#v", issues)
	}
	if !HasIssue(issues, "description_required") {
		t.Fatalf("expected description_required issue, got %#v", issues)
	}
}

func TestCategoryValidationAndCanonicalization(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: sample-skill\ndescription: Test\ncategory:  Data   Science \n---\nBody.\n")
	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	if bundle.Frontmatter.Category != "Data Science" {
		t.Fatalf("category whitespace was not normalized: %q", bundle.Frontmatter.Category)
	}
	if HasIssue(Validate(bundle), "category_required") {
		t.Fatal("a populated category must not trigger category_required")
	}
	missingDir := t.TempDir()
	writeFile(t, filepath.Join(missingDir, "SKILL.md"), "---\nname: missing-category\ndescription: Test\n---\nBody.\n")
	missing, err := LoadBundle(missingDir)
	if err != nil {
		t.Fatalf("LoadBundle missing category: %v", err)
	}
	if !HasIssue(Validate(missing), "category_required") {
		t.Fatal("missing category should produce a validation warning")
	}
	other := &Bundle{Frontmatter: Frontmatter{Category: "data science"}}
	NormalizeCategories([]*Bundle{bundle, other})
	if other.Frontmatter.Category != "Data Science" {
		t.Fatalf("category spelling was not snapped to existing spelling: %q", other.Frontmatter.Category)
	}
}

func TestValidateSkillNameAcceptsValidAndRejectsInvalidNames(t *testing.T) {
	valid := []string{"repo-review", "a", "my-skill-1", "opencode-123"}
	for _, name := range valid {
		if err := ValidateSkillName(name); err != nil {
			t.Fatalf("expected %q to be valid, got %v", name, err)
		}
	}
	invalid := []string{"", "Bad_Name", "evil/name", "../evil", "/etc/evil", "..", "skill.name", "UPPER"}
	for _, name := range invalid {
		if err := ValidateSkillName(name); err == nil {
			t.Fatalf("expected %q to be invalid", name)
		}
	}
}

func TestValidateSkillNameRejectsConsecutiveAndTrailingHyphens(t *testing.T) {
	// Previously accepted by the loose pattern, now rejected per the
	// agentskills spec: no consecutive, leading, or trailing hyphens, max 64.
	invalid := []string{"a--b", "ab-", "-ab", "pdf--processing", "pdf-", strings.Repeat("a", MaxNameLength+1)}
	for _, name := range invalid {
		if err := ValidateSkillName(name); err == nil {
			t.Errorf("expected %q to be invalid", name)
		}
	}
	valid := []string{"a-b", "ab", "pdf-processing", strings.Repeat("a", MaxNameLength)}
	for _, name := range valid {
		if err := ValidateSkillName(name); err != nil {
			t.Errorf("expected %q to be valid, got %v", name, err)
		}
	}
}

func TestValidateNameTooLongAndDirMismatch(t *testing.T) {
	// A name longer than 64 characters yields name_too_long.
	longDir := filepath.Join(t.TempDir(), strings.Repeat("a", MaxNameLength+1))
	writeFile(t, filepath.Join(longDir, "SKILL.md"), fmt.Sprintf("---\nname: %s\ndescription: test\n---\nBody\n", strings.Repeat("a", MaxNameLength+1)))
	longBundle, err := LoadBundle(longDir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	longIssues := Validate(longBundle)
	if !HasIssue(longIssues, "name_too_long") {
		t.Fatalf("expected name_too_long issue, got %#v", longIssues)
	}

	// A valid name whose directory is named differently yields
	// name_dir_mismatch.
	mismatchDir := filepath.Join(t.TempDir(), "other-dir")
	writeFile(t, filepath.Join(mismatchDir, "SKILL.md"), "---\nname: real-name\ndescription: test\n---\nBody\n")
	mismatchBundle, err := LoadBundle(mismatchDir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	mismatchIssues := Validate(mismatchBundle)
	mismatch := issueByCode(mismatchIssues, "name_dir_mismatch")
	if mismatch == nil {
		t.Fatalf("expected name_dir_mismatch issue, got %#v", mismatchIssues)
	}
	if mismatch.Severity != "error" {
		t.Errorf("name_dir_mismatch severity: want error, got %q", mismatch.Severity)
	}

	// A matching directory name validates clean.
	matchDir := filepath.Join(t.TempDir(), "real-name")
	writeFile(t, filepath.Join(matchDir, "SKILL.md"), "---\nname: real-name\ndescription: test\n---\nBody\n")
	matchBundle, err := LoadBundle(matchDir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	if HasIssue(Validate(matchBundle), "name_dir_mismatch") {
		t.Fatalf("unexpected name_dir_mismatch for matching dir, got %#v", Validate(matchBundle))
	}
}

func TestLoadBundleResourceInventory(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "inventory-skill")
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: inventory-skill\ndescription: test\n---\nBody\n")
	script := "#!/bin/sh\necho hi\n"
	writeFile(t, filepath.Join(dir, "scripts", "run.sh"), script)
	if err := os.Chmod(filepath.Join(dir, "scripts", "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}
	writeFile(t, filepath.Join(dir, "data", "notes.txt"), "hello")
	big := make([]byte, MaxResourceSize+1)
	if err := os.WriteFile(filepath.Join(dir, "data", "big.bin"), big, 0o644); err != nil {
		t.Fatal(err)
	}
	writeFile(t, filepath.Join(dir, "symskills.toml"), "[skill]\nname = \"inventory-skill\"\n")

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}

	byPath := map[string]Resource{}
	for _, r := range bundle.Resources {
		byPath[r.Path] = r
	}
	if len(byPath) != 4 {
		t.Fatalf("expected 4 resources, got %d: %#v", len(byPath), byPath)
	}
	if _, ok := byPath["SKILL.md"]; ok {
		t.Fatal("SKILL.md must not be part of the resource inventory")
	}
	run, ok := byPath["scripts/run.sh"]
	if !ok {
		t.Fatalf("missing scripts/run.sh in inventory: %#v", byPath)
	}
	if !run.Executable {
		t.Errorf("scripts/run.sh should be executable, got %#v", run)
	}
	if run.Size != int64(len(script)) {
		t.Errorf("scripts/run.sh size: want %d, got %d", len(script), run.Size)
	}
	if run.Mode != "0755" {
		t.Errorf("scripts/run.sh mode: want 0755, got %q", run.Mode)
	}
	if notes, ok := byPath["data/notes.txt"]; !ok || notes.Executable {
		t.Errorf("data/notes.txt should be present and non-executable: %#v", byPath)
	}
	if bigRes, ok := byPath["data/big.bin"]; !ok || bigRes.Size != MaxResourceSize+1 {
		t.Errorf("data/big.bin should be present with size %d: %#v", MaxResourceSize+1, byPath)
	}
	if _, ok := byPath["symskills.toml"]; !ok {
		t.Errorf("symskills.toml should be inventoried as a resource: %#v", byPath)
	}
}

func TestValidateResourceIssues(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "exec-skill")
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: exec-skill\ndescription: test\n---\nBody\n")
	writeFile(t, filepath.Join(dir, "run.sh"), "#!/bin/sh\necho hi\n")
	if err := os.Chmod(filepath.Join(dir, "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "big.bin"), make([]byte, MaxResourceSize+1), 0o644); err != nil {
		t.Fatal(err)
	}

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	issues := Validate(bundle)

	execIssue := issueByCode(issues, "resource_executable")
	if execIssue == nil {
		t.Fatalf("expected resource_executable issue, got %#v", issues)
	}
	if execIssue.Severity != "warning" {
		t.Errorf("resource_executable severity: want warning, got %q", execIssue.Severity)
	}
	if execIssue.Path != "run.sh" {
		t.Errorf("resource_executable path: want run.sh, got %q", execIssue.Path)
	}

	largeIssue := issueByCode(issues, "resource_too_large")
	if largeIssue == nil {
		t.Fatalf("expected resource_too_large issue, got %#v", issues)
	}
	if largeIssue.Severity != "warning" {
		t.Errorf("resource_too_large severity: want warning, got %q", largeIssue.Severity)
	}
	if largeIssue.Path != "big.bin" {
		t.Errorf("resource_too_large path: want big.bin, got %q", largeIssue.Path)
	}
	if !strings.Contains(largeIssue.Message, fmt.Sprint(MaxResourceSize)) {
		t.Errorf("resource_too_large message should name the limit, got %q", largeIssue.Message)
	}
}

func TestValidateDescriptionAndBodyLengthLimits(t *testing.T) {
	// The bundle directory must match the frontmatter name so no
	// name_dir_mismatch issue disturbs the length assertions.
	dir := filepath.Join(t.TempDir(), "long-skill")
	desc := strings.Repeat("d", MaxDescriptionLength+1)
	body := strings.Repeat("b", MaxBodyLength+1)
	// No trailing newline: the parsed body must equal the content exactly.
	writeFile(t, filepath.Join(dir, "SKILL.md"), fmt.Sprintf("---\nname: long-skill\ndescription: %s\n---\n\n%s", desc, body))

	bundle, err := LoadBundle(dir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	issues := Validate(bundle)

	descIssue := issueByCode(issues, "description_too_long")
	if descIssue == nil {
		t.Fatalf("expected description_too_long issue, got %#v", issues)
	}
	if descIssue.Severity != "error" {
		t.Errorf("description_too_long severity: want error, got %q", descIssue.Severity)
	}
	if !strings.Contains(descIssue.Message, "1024") || !strings.Contains(descIssue.Message, fmt.Sprint(MaxDescriptionLength+1)) {
		t.Errorf("description_too_long message should name actual and permitted length, got %q", descIssue.Message)
	}

	bodyIssue := issueByCode(issues, "body_too_long")
	if bodyIssue == nil {
		t.Fatalf("expected body_too_long issue, got %#v", issues)
	}
	if bodyIssue.Severity != "warning" {
		t.Errorf("body_too_long severity: want warning, got %q", bodyIssue.Severity)
	}
	if !strings.Contains(bodyIssue.Message, "50000") || !strings.Contains(bodyIssue.Message, fmt.Sprint(MaxBodyLength+1)) {
		t.Errorf("body_too_long message should name actual and permitted length, got %q", bodyIssue.Message)
	}

	// A bundle exactly at the limits stays clean.
	okDir := filepath.Join(t.TempDir(), "limit-skill")
	writeFile(t, filepath.Join(okDir, "SKILL.md"), fmt.Sprintf("---\nname: limit-skill\ndescription: %s\n---\n\n%s", strings.Repeat("d", MaxDescriptionLength), strings.Repeat("b", MaxBodyLength)))
	okBundle, err := LoadBundle(okDir)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	if HasIssue(Validate(okBundle), "description_too_long") || HasIssue(Validate(okBundle), "body_too_long") {
		t.Fatalf("bundle at the length limits should validate clean, got %#v", Validate(okBundle))
	}
}

func issueByCode(issues []Issue, code string) *Issue {
	for i := range issues {
		if issues[i].Code == code {
			return &issues[i]
		}
	}
	return nil
}

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
		if r.Status == BatchImported {
			imported[r.Name] = r
		} else if r.Status == BatchFailed {
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

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
