// Package vcs implements per-skill git versioning of the skill library
// (#118). Every skill in the managed library owns an independent git
// repository; symskills initializes it on import and commits automatically
// after every library write it performs.
//
// All repository access goes through the git binary — no git library is
// vendored and go.mod is never touched for this feature. When the git
// binary is missing, every operation degrades to ErrUnavailable and the
// caller is expected to continue without versioning (the operation itself
// still succeeds).
package vcs

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// ErrUnavailable reports that git cannot be run because the binary is
// missing from PATH. Callers must treat this as a non-fatal degradation.
var ErrUnavailable = errors.New("git binary not available")

// commitName and commitEmail are the identity used for symskills-made
// commits. They are applied per-invocation with -c flags, so the user's
// own git configuration is never modified and user commits keep their
// identity.
const (
	commitName  = "symskills"
	commitEmail = "symskills@localhost"
)

// Available reports whether the git binary can be found on PATH.
func Available() bool {
	_, err := exec.LookPath("git")
	return err == nil
}

// gitEnv returns the environment every git invocation runs with. Beyond
// disabling terminal prompts it pins the locale to C: symskills matches
// git's human-readable messages in a few places (an empty initial commit,
// a repository without commits), and those strings are translated on a
// developer machine with a non-English locale. Pinning the locale here
// keeps behaviour identical everywhere instead of depending on the
// caller's environment. LANGUAGE is cleared too because gettext consults
// it ahead of LC_ALL when the locale is not C.
func gitEnv() []string {
	return append(os.Environ(),
		"GIT_TERMINAL_PROMPT=0",
		"LC_ALL=C",
		"LANG=C",
		"LANGUAGE=",
	)
}

// runGit runs git in dir with args and returns its combined output. It
// returns ErrUnavailable when git is missing; any non-zero exit becomes
// an error that carries the command output for diagnostics.
func runGit(dir string, args ...string) (string, error) {
	path, err := exec.LookPath("git")
	if err != nil {
		return "", ErrUnavailable
	}
	cmd := exec.Command(path, args...)
	cmd.Dir = dir
	cmd.Env = gitEnv()
	var out bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &out
	if err := cmd.Run(); err != nil {
		return out.String(), fmt.Errorf("git %s: %w: %s", strings.Join(args, " "), err, strings.TrimSpace(out.String()))
	}
	return out.String(), nil
}

// commitFlags returns the -c overrides applied to every symskills commit.
// Signing is force-disabled so a user's commit.gpgsign setting cannot make
// automatic commits fail when no key is available.
func commitFlags() []string {
	return []string{
		"-c", "user.name=" + commitName,
		"-c", "user.email=" + commitEmail,
		"-c", "commit.gpgsign=false",
	}
}

// IsRepo reports whether dir contains a working git repository.
func IsRepo(dir string) bool {
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		return false
	}
	if _, err := runGit(dir, "rev-parse", "--git-dir"); err != nil {
		return false
	}
	return true
}

// Init creates a git repository at dir and records an initial commit of
// the current tree. It reports created=false when dir is already a
// repository, so calling it unconditionally after a write is safe. A tree
// with no files produces a repository with no commits yet (the initial
// commit is made lazily by Commit once content exists).
func Init(dir string) (bool, error) {
	if IsRepo(dir) {
		return false, nil
	}
	if _, err := runGit(dir, "init"); err != nil {
		return false, err
	}
	if _, err := runGit(dir, "add", "-A"); err != nil {
		return false, err
	}
	if _, err := runGit(dir, append(commitFlags(), "commit", "-m", "import: initial library import")...); err != nil {
		// An empty tree cannot be committed; the repo is still valid and
		// Commit() will create the first commit once content exists.
		if !strings.Contains(err.Error(), "nothing to commit") {
			return false, err
		}
	}
	return true, nil
}

// Commit stages every change in the working tree and creates one commit
// with the given message ("<operation>: <summary>"). A clean tree is a
// no-op that returns ""; otherwise the full hash of the new commit is
// returned. History is never rewritten: this only ever adds a commit on
// top of HEAD, so user commits are preserved.
func Commit(dir, message string) (string, error) {
	dirty, err := Dirty(dir)
	if err != nil {
		return "", err
	}
	if !dirty {
		return "", nil
	}
	if _, err := runGit(dir, "add", "-A"); err != nil {
		return "", err
	}
	if _, err := runGit(dir, append(commitFlags(), "commit", "-m", message)...); err != nil {
		return "", err
	}
	return Head(dir)
}

// Head returns the full hash of the current HEAD.
func Head(dir string) (string, error) {
	out, err := runGit(dir, "rev-parse", "HEAD")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(out), nil
}

// Dirty reports whether the working tree has staged, unstaged or
// untracked changes.
func Dirty(dir string) (bool, error) {
	out, err := runGit(dir, "status", "--porcelain")
	if err != nil {
		return false, err
	}
	return strings.TrimSpace(out) != "", nil
}

// CommitInfo is one entry of a skill's history (#119). Files lists the
// paths the commit changed relative to its parent (every tracked file for
// the initial commit).
type CommitInfo struct {
	// Hash is the full commit hash.
	Hash string `json:"revision"`
	// Timestamp is the commit's author date in RFC3339 format.
	Timestamp string `json:"timestamp"`
	// Subject is the commit message first line.
	Subject string `json:"subject"`
	// Operation is the operation label parsed from the subject prefix
	// (import, update, restore), or "unknown" for hand-made commits.
	Operation string `json:"operation"`
	// Files are the paths the commit changed.
	Files []string `json:"files"`
}

// History returns the up to limit most recent commits of the repository
// at dir, newest first. The operation label is parsed from the symskills
// auto-commit message ("<operation>: ..."), falling back to "unknown" for
// any other subject. A repository without commits yields an empty list.
func History(dir string, limit int) ([]CommitInfo, error) {
	if limit <= 0 {
		limit = 20
	}
	// Each record is separated by \x1e and its fields by \x1f so subjects
	// and file names can never collide with the delimiters.
	out, err := runGit(dir, "log", "--no-color", fmt.Sprintf("-n%d", limit), "--name-only", "--format=%x1e%H%x1f%aI%x1f%s")
	if err != nil {
		if strings.Contains(err.Error(), "does not have any commits yet") {
			return []CommitInfo{}, nil
		}
		return nil, err
	}
	commits := []CommitInfo{}
	for _, chunk := range strings.Split(out, "\x1e") {
		chunk = strings.Trim(chunk, "\n\r")
		if chunk == "" {
			continue
		}
		lines := strings.Split(chunk, "\n")
		fields := strings.Split(lines[0], "\x1f")
		if len(fields) < 3 {
			continue
		}
		files := []string{}
		for _, f := range lines[1:] {
			if f = strings.TrimRight(f, "\r"); f != "" {
				files = append(files, f)
			}
		}
		commits = append(commits, CommitInfo{
			Hash:      fields[0],
			Timestamp: fields[1],
			Subject:   fields[2],
			Operation: operationFromSubject(fields[2]),
			Files:     files,
		})
	}
	return commits, nil
}

// operationFromSubject extracts the operation label from a symskills
// auto-commit subject ("<operation>: <summary>"), falling back to
// "unknown" for hand-made commits and anything unrecognized.
func operationFromSubject(subject string) string {
	op, _, ok := strings.Cut(subject, ":")
	if !ok {
		return "unknown"
	}
	switch op {
	case "import", "update", "restore":
		return op
	}
	return "unknown"
}

// Resolve expands a revision expression (full hash, prefix, HEAD, HEAD~1,
// ...) to the full commit hash, verifying that it names a commit.
func Resolve(dir, rev string) (string, error) {
	out, err := runGit(dir, "rev-parse", "--verify", rev+"^{commit}")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(out), nil
}

// ShowFile returns the content of the tracked file at path as of the
// revision rev.
func ShowFile(dir, rev, path string) (string, error) {
	return runGit(dir, "show", "--no-color", rev+":"+path)
}

// TreeFiles returns the paths tracked at revision rev, in tree order.
func TreeFiles(dir, rev string) ([]string, error) {
	out, err := runGit(dir, "ls-tree", "-r", "--name-only", rev)
	if err != nil {
		return nil, err
	}
	files := []string{}
	for _, line := range strings.Split(out, "\n") {
		if line = strings.TrimRight(line, "\r"); strings.TrimSpace(line) != "" {
			files = append(files, line)
		}
	}
	return files, nil
}

// Diff returns a unified diff of the revision rev against the current
// working tree (tracked files only, no color).
func Diff(dir, rev string) (string, error) {
	return runGit(dir, "diff", "--no-color", rev)
}

// ChangedFiles returns the paths that differ between the revision rev and
// the current working tree (tracked files only).
func ChangedFiles(dir, rev string) ([]string, error) {
	out, err := runGit(dir, "diff", "--name-only", "--no-color", rev)
	if err != nil {
		return nil, err
	}
	files := []string{}
	for _, line := range strings.Split(out, "\n") {
		if line = strings.TrimRight(line, "\r"); strings.TrimSpace(line) != "" {
			files = append(files, line)
		}
	}
	return files, nil
}

// ExtractRev materializes the tracked tree at revision rev into dst,
// which must not already contain the files (a fresh temp dir). The
// repository itself is never touched — extraction reads the object store
// via `git archive` and unpacks it with the standard library's tar
// reader, so no external tar binary is needed. Symlinks are recreated as
// symlinks, regular files keep their mode bits.
