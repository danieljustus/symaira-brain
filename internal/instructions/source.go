// Package instructions manages the canonical instructions source and syncs
// it into harness-specific files through idempotent managed blocks.
package instructions

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/xdg"
)

const (
	// GlobalFileName is the basename of the global instructions file in
	// the XDG config directory (~/.config/symbrain/instructions.md).
	GlobalFileName = "instructions.md"

	// ProjectDirName is the project-local directory that holds
	// project-specific instructions (<project>/.symbrain/).
	ProjectDirName = ".symbrain"

	// ProjectFileName is the basename of the project-local instructions
	// file (<project>/.symbrain/instructions.md).
	ProjectFileName = "instructions.md"
)

// Source resolves and loads the canonical instructions content from the
// global and optional project-local files.  When both exist the project
// content is appended after the global content so the project can override
// or extend the global instructions.
type Source struct {
	// GlobalPath is the resolved path to the global instructions file.
	GlobalPath string
	// ProjectPath is the resolved path to the project-local instructions
	// file, or empty when no project directory was provided.
	ProjectPath string
}

// NewSource resolves the instruction file paths.  projectDir may be empty;
// when set it is used as the base for the project-local instructions file.
func NewSource(projectDir string) *Source {
	globalPath := filepath.Join(xdg.ConfigDir(), GlobalFileName)

	s := &Source{
		GlobalPath: globalPath,
	}
	if projectDir != "" {
		s.ProjectPath = filepath.Join(projectDir, ProjectDirName, ProjectFileName)
	}
	return s
}

// Content returns the merged instructions content.  The global file is
// loaded first (if present); the project file is appended after it (if
// present).  When neither file exists an empty string is returned with a
// nil error — callers treat this as "no instructions to sync".
func (s *Source) Content() (string, error) {
	var parts []string

	if data, err := os.ReadFile(s.GlobalPath); err == nil {
		parts = append(parts, string(data))
	} else if !os.IsNotExist(err) {
		return "", fmt.Errorf("instructions: read global %s: %w", s.GlobalPath, err)
	}

	if s.ProjectPath != "" {
		if data, err := os.ReadFile(s.ProjectPath); err == nil {
			parts = append(parts, string(data))
		} else if !os.IsNotExist(err) {
			return "", fmt.Errorf("instructions: read project %s: %w", s.ProjectPath, err)
		}
	}

	// Concatenate global and project content.  Project content is appended
	// directly after global content without an extra separator — each file's
	// own trailing newline provides the separation.
	var buf strings.Builder
	for _, part := range parts {
		buf.WriteString(part)
	}
	return buf.String(), nil
}
