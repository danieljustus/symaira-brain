package vcs

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// makeHistoryRepo builds a repo with two symskills-style commits: the
// initial import of v1 and an update commit carrying v2. Returns the dir
// and the full hash of the initial commit.
func makeHistoryRepo(t *testing.T) (string, string) {
	t.Helper()
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatalf("Init: %v", err)
	}
	first, err := Head(dir)
	if err != nil {
		t.Fatalf("Head: %v", err)
	}
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nBody v2.\n")
	writeFile(t, filepath.Join(dir, "references", "details.md"), "Details v2.\n")
	writeFile(t, filepath.Join(dir, "scripts", "run.sh"), "#!/bin/sh\necho v2\n")
	if _, err := Commit(dir, "update: skill vcs-test from /tmp/src"); err != nil {
		t.Fatalf("Commit: %v", err)
	}
	return dir, first
}

func TestHistoryListsCommitsWithOperations(t *testing.T) {
	dir, first := makeHistoryRepo(t)

	history, err := History(dir, 20)
	if err != nil {
		t.Fatalf("History: %v", err)
	}
	if len(history) != 2 {
		t.Fatalf("expected 2 commits, got %d: %#v", len(history), history)
	}
	// Newest first.
	if history[0].Operation != "update" {
		t.Errorf("expected newest commit operation update, got %q", history[0].Operation)
	}
	if history[1].Operation != "import" {
		t.Errorf("expected oldest commit operation import, got %q", history[1].Operation)
	}
	if history[1].Hash != first {
		t.Errorf("expected oldest hash %s, got %s", first, history[1].Hash)
	}
	if history[0].Timestamp == "" {
		t.Error("expected a timestamp on the update commit")
	}
	if !strings.Contains(history[0].Subject, "update:") {
		t.Errorf("expected update subject, got %q", history[0].Subject)
	}
	// The update commit names its changed files.
	if len(history[0].Files) != 3 {
		t.Fatalf("expected 3 changed files on the update commit, got %#v", history[0].Files)
	}
	want := map[string]bool{"SKILL.md": true, "references/details.md": true, "scripts/run.sh": true}
	for _, f := range history[0].Files {
		if !want[f] {
			t.Errorf("unexpected changed file %q", f)
		}
	}
	// The initial commit contains the whole v1 tree.
	if len(history[1].Files) != 2 {
		t.Fatalf("expected 2 files on the import commit, got %#v", history[1].Files)
	}
}

func TestHistoryLimit(t *testing.T) {
	dir, _ := makeHistoryRepo(t)
	history, err := History(dir, 1)
	if err != nil {
		t.Fatalf("History: %v", err)
	}
	if len(history) != 1 {
		t.Fatalf("expected 1 commit with limit 1, got %d", len(history))
	}
	if history[0].Operation != "update" {
		t.Errorf("expected the newest commit, got %q", history[0].Operation)
	}
}

func TestHistoryOperationFallsBackToUnknown(t *testing.T) {
	dir, _ := makeHistoryRepo(t)
	// A hand-made user commit with a non-symskills subject.
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nHand-made edit.\n")
	if _, err := runGit(dir, append(commitFlags(), "commit", "-am", "user: hand-made edit")...); err != nil {
		t.Fatalf("user commit: %v", err)
	}
	history, err := History(dir, 20)
	if err != nil {
		t.Fatal(err)
	}
	if len(history) != 3 {
		t.Fatalf("expected 3 commits, got %d", len(history))
	}
	if history[0].Operation != "unknown" {
		t.Errorf("expected hand-made commit to fall back to unknown, got %q", history[0].Operation)
	}
}

func TestHistoryEmptyRepo(t *testing.T) {
	dir := t.TempDir()
	if _, err := runGit(dir, "init"); err != nil {
		t.Fatal(err)
	}
	history, err := History(dir, 20)
	if err != nil {
		t.Fatalf("History on empty repo: %v", err)
	}
	if len(history) != 0 {
		t.Fatalf("expected empty history, got %#v", history)
	}
}

func TestResolve(t *testing.T) {
	dir, first := makeHistoryRepo(t)
	resolved, err := Resolve(dir, "HEAD~1")
	if err != nil {
		t.Fatalf("Resolve HEAD~1: %v", err)
	}
	if resolved != first {
		t.Errorf("expected HEAD~1 to resolve to %s, got %s", first, resolved)
	}
	// Short prefixes work.
	prefix, err := Resolve(dir, first[:10])
	if err != nil {
		t.Fatalf("Resolve prefix: %v", err)
	}
	if prefix != first {
		t.Errorf("expected prefix to resolve to %s, got %s", first, prefix)
	}
	if _, err := Resolve(dir, "definitely-not-a-rev"); err == nil {
		t.Fatal("expected Resolve to fail on an unknown revision")
	}
}

func TestShowFileAndTreeAtRev(t *testing.T) {
	dir, first := makeHistoryRepo(t)
	content, err := ShowFile(dir, first, "SKILL.md")
	if err != nil {
		t.Fatalf("ShowFile: %v", err)
	}
	if !strings.Contains(content, "Body v1.") {
		t.Errorf("expected v1 body at the initial commit, got %q", content)
	}
	files, err := TreeFiles(dir, first)
	if err != nil {
		t.Fatalf("TreeFiles: %v", err)
	}
	if len(files) != 2 {
		t.Fatalf("expected 2 files at the initial commit, got %#v", files)
	}
	if _, err := ShowFile(dir, first, "scripts/run.sh"); err == nil {
		t.Fatal("expected ShowFile to fail for a file not present at the revision")
	}
}

func TestDiffAndChangedFiles(t *testing.T) {
	dir, first := makeHistoryRepo(t)
	diff, err := Diff(dir, first)
	if err != nil {
		t.Fatalf("Diff: %v", err)
	}
	if !strings.Contains(diff, "Body v2.") || !strings.Contains(diff, "-Body v1.") {
		t.Errorf("expected v1->v2 diff content, got %q", diff)
	}
	changed, err := ChangedFiles(dir, first)
	if err != nil {
		t.Fatalf("ChangedFiles: %v", err)
	}
	if len(changed) != 3 {
		t.Fatalf("expected 3 changed files, got %#v", changed)
	}
}

func TestExtractRevMaterializesTree(t *testing.T) {
	dir, first := makeHistoryRepo(t)
	dst := t.TempDir()
	if err := ExtractRev(dir, first, dst); err != nil {
		t.Fatalf("ExtractRev: %v", err)
	}
	data, err := os.ReadFile(filepath.Join(dst, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "Body v1.") {
		t.Errorf("expected v1 content in extraction, got %q", data)
	}
	if _, err := os.Stat(filepath.Join(dst, "scripts", "run.sh")); err == nil {
		t.Error("expected scripts/run.sh to be absent from the v1 extraction")
	}
	if _, err := os.Stat(filepath.Join(dst, ".git")); err == nil {
		t.Error("extraction must never contain .git")
	}
}

// TestRestoreCreatesForwardCommitPreservingHistory locks the #119
// acceptance criterion: restoring returns the files to the requested
// state and leaves the intermediate history intact (verified with git
// log).
func TestRestoreCreatesForwardCommitPreservingHistory(t *testing.T) {
	dir, first := makeHistoryRepo(t)

	// Restore the initial revision's tree.
	head, err := Restore(dir, mustExtract(t, dir, first), "restore: skill vcs-test to "+first)
	if err != nil {
		t.Fatalf("Restore: %v", err)
	}
	if head == "" {
		t.Fatal("expected a restore commit")
	}
	data, err := os.ReadFile(filepath.Join(dir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "Body v1.") {
		t.Errorf("expected v1 content after restore, got %q", data)
	}
	if _, err := os.Stat(filepath.Join(dir, "scripts", "run.sh")); err == nil {
		t.Error("expected scripts/run.sh to be gone after restore to v1")
	}
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Errorf("restore must preserve .git: %v", err)
	}
	// History intact: init, update, restore — three commits, the update
	// commit still in the middle.
	log, err := runGit(dir, "log", "--format=%H")
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(log), "\n")
	if len(lines) != 3 {
		t.Fatalf("expected 3 commits (import, update, restore), got %d: %v", len(lines), lines)
	}
	if lines[2] != first {
		t.Errorf("initial commit was rewritten: want %s at the root, got %s", first, lines[2])
	}
	if lines[0] != head {
		t.Errorf("expected restore commit %s to be HEAD, got %s", head, lines[0])
	}
	// The intermediate update commit still holds v2.
	prev, err := runGit(dir, "show", lines[1]+":SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(prev, "Body v2.") {
		t.Errorf("intermediate history lost v2: %q", prev)
	}
}

func TestRestoreNoopWhenTreeIdentical(t *testing.T) {
	dir, _ := makeHistoryRepo(t)
	head, err := Head(dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Restore(dir, mustExtract(t, dir, head), "restore: no-op")
	if err != nil {
		t.Fatalf("Restore: %v", err)
	}
	if got != "" {
		t.Fatalf("expected no-op restore to return empty hash, got %q", got)
	}
}

func TestRestoreRefusesNonRepo(t *testing.T) {
	dir := t.TempDir()
	if _, err := Restore(dir, dir, "restore: x"); err == nil {
		t.Fatal("expected Restore to refuse a non-repository directory")
	}
}

// mustExtract materializes the tree at rev into a fresh temp dir and
// returns it, failing the test on error.
func mustExtract(t *testing.T, dir, rev string) string {
	t.Helper()
	dst := t.TempDir()
	if err := ExtractRev(dir, rev, dst); err != nil {
		t.Fatalf("ExtractRev: %v", err)
	}
	return dst
}

// TestExtractRevRefusesEscapingSymlink guards the archive-extraction
// traversal checks (CodeQL: unsanitized archive entry / symlink creation):
// a tracked symlink whose linkname escapes the destination root must be
// refused, never materialized.
func TestExtractRevRefusesEscapingSymlink(t *testing.T) {
	for _, tc := range []struct {
		name     string
		linkname string
	}{
		{"absolute", "/etc"},
		{"dotdot", "../../etc"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dir := makeSkillDir(t)
			if _, err := Init(dir); err != nil {
				t.Fatal(err)
			}
			writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: evil\ndescription: x\n---\n")
			if err := os.Symlink(tc.linkname, filepath.Join(dir, "escape")); err != nil {
				t.Fatal(err)
			}
			if _, err := Commit(dir, "add escaping symlink"); err != nil {
				t.Fatal(err)
			}
			head, err := Head(dir)
			if err != nil {
				t.Fatal(err)
			}
			dst := t.TempDir()
			err = ExtractRev(dir, head, dst)
			if err == nil {
				t.Fatalf("expected ExtractRev to refuse linkname %q", tc.linkname)
			}
			if !strings.Contains(err.Error(), "escapes destination") && !strings.Contains(err.Error(), "absolute or empty linkname") {
				t.Fatalf("expected escape error for linkname %q, got: %v", tc.linkname, err)
			}
			if _, statErr := os.Lstat(filepath.Join(dst, "escape")); !os.IsNotExist(statErr) {
				t.Fatalf("escaping symlink must not be materialized in dst")
			}
		})
	}
}
