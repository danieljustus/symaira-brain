// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
)

// HasIssue reports whether issues contains code. It is public for tests and CLI checks.
func HasIssue(issues []Issue, code string) bool {
	return slices.ContainsFunc(issues, func(issue Issue) bool {
		return issue.Code == code
	})
}

// ImportSkill copies an existing skill directory into a managed library.
func ImportSkill(srcRoot, libraryDir string) (ImportResult, error) {
	return ImportSkillOpts(srcRoot, libraryDir, ImportOptions{VCSEnabled: true})
}

// ImportOptions controls how ImportSkillOpts performs an import.
type ImportOptions struct {
	// VCSEnabled enables per-skill git versioning (#118): after a
	// successful copy the skill gets its own repository with an initial
	// commit, and updates produce an automatic commit. Defaults to true.
	VCSEnabled bool
	// Update re-imports over an existing library skill instead of
	// refusing, replacing the tracked tree and recording a new commit.
	Update bool
}

// ImportSkillOpts copies an existing skill directory into a managed
// library, honoring ImportOptions. With Update set, an existing library
// entry is replaced in place (its .git repository survives the swap) and
// the change is committed; without it, a pre-existing entry is an error.
// When versioning is enabled and git is available, a fresh import ends
// with exactly one commit containing the full skill. Versioning failures
// after a successful copy degrade to a VCSWarning, never an import error.
func ImportSkillOpts(srcRoot, libraryDir string, opts ImportOptions) (ImportResult, error) {
	bundle, err := LoadBundle(srcRoot)
	if err != nil {
		return ImportResult{}, err
	}
	name := bundle.Frontmatter.Name
	if name == "" {
		return ImportResult{}, fmt.Errorf("cannot import skill without name")
	}
	dst := filepath.Join(libraryDir, name)
	exists := false
	if _, err := os.Stat(dst); err == nil {
		exists = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return ImportResult{}, err
	}
	if exists && !opts.Update {
		return ImportResult{}, fmt.Errorf("skill %q already exists in library", name)
	}
	operation := "import"
	if exists {
		operation = "update"
		if err := replaceTreeContents(srcRoot, dst); err != nil {
			return ImportResult{}, err
		}
	} else {
		if err := fsutil.CopyTree(srcRoot, dst, func(rel string, d os.DirEntry) bool {
			return d.Name() == ".git" && d.IsDir()
		}); err != nil {
			return ImportResult{}, err
		}
	}
	result := ImportResult{Name: name, Path: dst}
	if !opts.VCSEnabled {
		return result, nil
	}
	// Versioning is best-effort: the copy already succeeded, so git
	// problems degrade to a warning instead of failing the import. A
	// missing git binary is reported by the CLI once per command, so it
	// stays silent here (ErrUnavailable) to avoid per-skill noise.
	if _, err := vcs.Init(dst); err != nil {
		if !errors.Is(err, vcs.ErrUnavailable) {
			result.VCSWarning = fmt.Sprintf("versioning unavailable for %s: %v", name, err)
		}
		return result, nil
	}
	if _, err := vcs.Commit(dst, fmt.Sprintf("%s: skill %s from %s", operation, name, srcRoot)); err != nil {
		if !errors.Is(err, vcs.ErrUnavailable) {
			result.VCSWarning = fmt.Sprintf("versioning degraded for %s: %v", name, err)
		}
	}
	return result, nil
}

// replaceTreeContents replaces every file of dst with a fresh copy of src,
// preserving dst/.git so versioning survives an update. The copy happens
// in a temporary sibling directory first so a failed copy never leaves
// dst half-written.
func replaceTreeContents(src, dst string) error {
	tmp, err := os.MkdirTemp(filepath.Dir(dst), ".import-tmp-*")
	if err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	if err := fsutil.CopyTree(src, tmp, func(rel string, d os.DirEntry) bool {
		return d.Name() == ".git" && d.IsDir()
	}); err != nil {
		return err
	}
	entries, err := os.ReadDir(dst)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Name() == ".git" {
			continue
		}
		if err := os.RemoveAll(filepath.Join(dst, entry.Name())); err != nil {
			return err
		}
	}
	tmpEntries, err := os.ReadDir(tmp)
	if err != nil {
		return err
	}
	for _, entry := range tmpEntries {
		if err := os.Rename(filepath.Join(tmp, entry.Name()), filepath.Join(dst, entry.Name())); err != nil {
			return err
		}
	}
	return nil
}

// isSkillDir reports whether path contains a SKILL.md file.
func isSkillDir(path string) bool {
	fi, err := os.Stat(filepath.Join(path, "SKILL.md"))
	return err == nil && fi.Mode().IsRegular()
}

// ImportSkills discovers and imports skill directories from a parent directory.
// If the given directory itself contains SKILL.md it falls back to a single-skill import.
// Otherwise it scans immediate subdirectories for SKILL.md and imports each one.
// It never fails entirely on a single bad subdirectory — per-item results are returned.
func ImportSkills(parentDir, libraryDir string) []BatchImportResult {
	return ImportSkillsOpts(parentDir, libraryDir, ImportOptions{VCSEnabled: true})
}

// ImportSkillsOpts is ImportSkills with ImportOptions (versioning toggle
// and update semantics) applied to every imported skill.
func ImportSkillsOpts(parentDir, libraryDir string, opts ImportOptions) []BatchImportResult {
	// If the directory itself is a skill, fall back to single import.
	if isSkillDir(parentDir) {
		res, err := ImportSkillOpts(parentDir, libraryDir, opts)
		if err != nil {
			return []BatchImportResult{{
				Name:   filepath.Base(parentDir),
				Status: BatchFailed,
				Error:  err.Error(),
			}}
		}
		return []BatchImportResult{{
			Name:       res.Name,
			Path:       res.Path,
			Status:     BatchImported,
			VCSWarning: res.VCSWarning,
		}}
	}

	entries, err := os.ReadDir(parentDir)
	if err != nil {
		return []BatchImportResult{{
			Name:   filepath.Base(parentDir),
			Status: BatchFailed,
			Error:  err.Error(),
		}}
	}

	var results []BatchImportResult
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		subdir := filepath.Join(parentDir, entry.Name())
		if !isSkillDir(subdir) {
			continue
		}
		res, err := ImportSkillOpts(subdir, libraryDir, opts)
		if err != nil {
			results = append(results, BatchImportResult{
				Name:   entry.Name(),
				Status: BatchFailed,
				Error:  err.Error(),
			})
			continue
		}
		if res.Name != "" && res.Path != "" {
			results = append(results, BatchImportResult{
				Name:       res.Name,
				Path:       res.Path,
				Status:     BatchImported,
				VCSWarning: res.VCSWarning,
			})
		}
	}
	return results
}
