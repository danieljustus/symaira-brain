// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"errors"
	"os"
	"path/filepath"
)

// ListLibrary returns loaded bundles under a library directory.
func ListLibrary(libraryDir string) ([]*Bundle, []Issue) {
	entries, err := os.ReadDir(libraryDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, []Issue{{Code: "library_read", Severity: "error", Message: err.Error(), Path: libraryDir}}
	}
	var bundles []*Bundle
	var issues []Issue
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		root := filepath.Join(libraryDir, entry.Name())
		fm, err := loadFrontmatterOnly(root)
		if err != nil {
			issues = append(issues, Issue{Code: "skill_load", Severity: "error", Message: err.Error(), Path: entry.Name()})
			continue
		}
		abs, err := filepath.Abs(root)
		if err != nil {
			issues = append(issues, Issue{Code: "skill_load", Severity: "error", Message: err.Error(), Path: entry.Name()})
			continue
		}
		bundles = append(bundles, &Bundle{Root: abs, Frontmatter: fm, Manifest: Manifest{Targets: map[string]TargetConfig{}}})
	}
	NormalizeCategories(bundles)
	return bundles, issues
}
