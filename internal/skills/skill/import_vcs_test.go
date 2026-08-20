package skill

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
)

// gitLogCount runs `git log --oneline` in dir via the vcs runner.
func gitLogCount(t *testing.T, dir string) int {
	t.Helper()
	out, err := runGitCommand(dir, "log", "--format=%s")
	if err != nil {
		t.Fatalf("git log: %v", err)
	}
	if strings.TrimSpace(out) == "" {
		return 0
	}
	return len(strings.Split(strings.TrimSpace(out), "\n"))
}

// runGitCommand shells out to git for assertions in tests.
func runGitCommand(dir string, args ...string) (string, error) {
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	out, err := cmd.Output()
	if err != nil {
		return "", err
	}
	return string(out), nil
}

func TestImportSkillInitializesGitRepoWithSingleCommit(t *testing.T) {
	if !vcs.Available() {
		t.Skip("git not available")
	}
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), "---\nname: vcs-import\ndescription: versioned import\n---\n\nBody.\n")
	writeFile(t, filepath.Join(src, "references", "details.md"), "Details.\n")

	dst := filepath.Join(t.TempDir(), "library")
	imported, err := ImportSkill(src, dst)
	if err != nil {
		t.Fatalf("ImportSkill: %v", err)
	}
	if imported.VCSWarning != "" {
		t.Fatalf("unexpected vcs warning: %s", imported.VCSWarning)
	}
	skillDir := filepath.Join(dst, "vcs-import")
	if !vcs.IsRepo(skillDir) {
		t.Fatal("expected a git repository after import")
	}
	if n := gitLogCount(t, skillDir); n != 1 {
		t.Fatalf("expected exactly one commit, got %d", n)
	}
	// The single commit must contain the full skill.
	out, err := runGitCommand(skillDir, "ls-tree", "-r", "--name-only", "HEAD")
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"SKILL.md", "references/details.md"} {
		if !strings.Contains(out, want) {
			t.Errorf("initial commit missing %s; tree:\n%s", want, out)
		}
	}
}

func TestImportSkillUpdateCreatesSecondCommit(t *testing.T) {
	if !vcs.Available() {
		t.Skip("git not available")
	}
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), "---\nname: vcs-update\ndescription: versioned update\n---\n\nBody v1.\n")
	dst := filepath.Join(t.TempDir(), "library")
	if _, err := ImportSkill(src, dst); err != nil {
		t.Fatal(err)
	}
	skillDir := filepath.Join(dst, "vcs-update")

	// Re-import refuses without Update.
	if _, err := ImportSkill(src, dst); err == nil {
		t.Fatal("expected duplicate import to fail without Update")
	}
	// Modify the source and re-import with Update.
	writeFile(t, filepath.Join(src, "SKILL.md"), "---\nname: vcs-update\ndescription: versioned update\n---\n\nBody v2.\n")
	res, err := ImportSkillOpts(src, dst, ImportOptions{VCSEnabled: true, Update: true})
	if err != nil {
		t.Fatalf("update import: %v", err)
	}
	if res.VCSWarning != "" {
		t.Fatalf("unexpected vcs warning: %s", res.VCSWarning)
	}
	if n := gitLogCount(t, skillDir); n != 2 {
		t.Fatalf("expected two commits after update, got %d", n)
	}
	if !vcs.IsRepo(skillDir) {
		t.Fatal("expected repository to survive the update swap")
	}
	// The on-disk state is v2 and the first commit still holds v1.
	data, err := os.ReadFile(filepath.Join(skillDir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "Body v2.") {
		t.Fatalf("expected updated body on disk, got %q", data)
	}
	first := mustGitOutput(t, skillDir, "rev-list", "--max-parents=0", "HEAD")
	v1, err := runGitCommand(skillDir, "show", strings.TrimSpace(first)+":SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(v1, "Body v1.") {
		t.Fatalf("previous state lost: first commit holds %q", v1)
	}
}

func TestImportSkillVCSDisabledWritesNoRepo(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), "---\nname: vcs-off\ndescription: unversioned import\n---\n\nBody.\n")
	dst := filepath.Join(t.TempDir(), "library")
	res, err := ImportSkillOpts(src, dst, ImportOptions{VCSEnabled: false})
	if err != nil {
		t.Fatalf("ImportSkillOpts: %v", err)
	}
	if res.VCSWarning != "" {
		t.Fatalf("unexpected warning: %s", res.VCSWarning)
	}
	if vcs.IsRepo(filepath.Join(dst, "vcs-off")) {
		t.Fatal("expected no repository when versioning is disabled")
	}
}

func TestImportSkillDegradesWhenGitMissing(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "SKILL.md"), "---\nname: vcs-degrade\ndescription: degraded import\n---\n\nBody.\n")
	dst := filepath.Join(t.TempDir(), "library")
	res, err := ImportSkill(src, dst)
	if err != nil {
		t.Fatalf("import must still succeed without git, got: %v", err)
	}
	// The missing-binary degradation is reported once by the CLI, so the
	// library layer stays silent; the import itself must be complete.
	if res.VCSWarning != "" {
		t.Fatalf("expected no per-skill warning for a missing git binary, got %q", res.VCSWarning)
	}
	if _, err := os.Stat(filepath.Join(dst, "vcs-degrade", "SKILL.md")); err != nil {
		t.Fatalf("imported files missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "vcs-degrade", ".git")); err == nil {
		t.Fatal("expected no repository when git is missing")
	}
}

func mustGitOutput(t *testing.T, dir string, args ...string) string {
	t.Helper()
	out, err := runGitCommand(dir, args...)
	if err != nil {
		t.Fatalf("git %v: %v", args, err)
	}
	return out
}
