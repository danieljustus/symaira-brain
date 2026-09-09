// Package instructions manages the canonical instructions source and syncs
// it into harness-specific files through idempotent managed blocks.
package instructions

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
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

	// MaxSourceFileBytes bounds one canonical source file. The limit applies
	// to bytes read from the already-open file handle, not a path re-open.
	MaxSourceFileBytes = 1 << 20
	// MaxSourceTotalBytes bounds the merged global and project sources.
	MaxSourceTotalBytes = 3 << 19
)

type sourceCapability struct {
	parent atomicParent
	name   string
}

// Source resolves and loads the canonical instructions content from the
// global and optional project-local files. It retains the containing directory
// capabilities obtained during construction, so a later replacement of an
// XDG/home/project parent cannot redirect a read. The exported paths remain
// for diagnostics and source-compatible callers; use NewSource or FromPaths
// so capabilities are retained from the start.
type Source struct {
	// GlobalPath is the resolved path to the global instructions file.
	GlobalPath string
	// ProjectPath is the resolved path to the project-local instructions
	// file, or empty when no project directory was provided.
	ProjectPath string

	global     *sourceCapability
	project    *sourceCapability
	globalErr  error
	projectErr error
	prepared   bool
}

// NewSource resolves the instruction file paths. projectDir may be empty;
// when set it is used as the base for the project-local instructions file.
// An absolute XDG_CONFIG_HOME takes precedence over the home fallback.
func NewSource(projectDir string) *Source {
	globalBase := os.Getenv("XDG_CONFIG_HOME")
	if !filepath.IsAbs(globalBase) {
		home, err := os.UserHomeDir()
		if err == nil {
			globalBase = filepath.Join(home, ".config")
		} else {
			globalBase = ".config"
		}
	}

	s := &Source{
		GlobalPath: filepath.Join(globalBase, "symbrain", GlobalFileName),
	}
	if projectDir != "" {
		s.ProjectPath = filepath.Join(projectDir, ProjectDirName, ProjectFileName)
	}
	s.prepare()
	return s
}

// FromPaths constructs a source from explicit paths and retains secure parent
// capabilities for both paths. It is useful for isolated callers and tests.
func FromPaths(globalPath, projectPath string) *Source {
	s := &Source{GlobalPath: globalPath, ProjectPath: projectPath}
	s.prepare()
	return s
}

func openSourceParent(filePath string) (atomicParent, error) {
	return openAtomicParent(filepath.Dir(filePath), ".", false)
}

func (s *Source) prepare() {
	s.global, s.globalErr = prepareSourceCapability(s.GlobalPath)
	if s.ProjectPath != "" {
		s.project, s.projectErr = prepareSourceCapability(s.ProjectPath)
	}
	s.prepared = true
}

func prepareSourceCapability(filePath string) (*sourceCapability, error) {
	if filePath == "" {
		return nil, nil
	}
	parent, err := openSourceParent(filePath)
	if err != nil {
		if os.IsNotExist(err) {
			// A missing source is allowed. Do not retry through a path that may
			// become a symlink after Source construction.
			return nil, nil
		}
		return nil, err
	}
	return &sourceCapability{parent: parent, name: filepath.Base(filePath)}, nil
}

// Close releases retained source directory capabilities.
func (s *Source) Close() error {
	var first error
	for _, capability := range []*sourceCapability{s.global, s.project} {
		if capability != nil && capability.parent != nil {
			if err := capability.parent.Close(); err != nil && first == nil {
				first = err
			}
			capability.parent = nil
		}
	}
	return first
}

// Content returns the merged instructions content. The global file is
// loaded first (if present); the project file is appended after it (if
// present). When neither file exists an empty string is returned with a nil
// error. Existing sources must be regular files and are read through one
// bounded, already-open file handle.
func (s *Source) Content() (string, error) {
	if !s.prepared {
		// Keep compatibility with callers that construct Source literals while still
		// acquiring capabilities before any read. A nil capability is never a
		// reason to fall back to a pathname read.
		s.prepare()
	}
	var merged []byte
	for _, item := range []struct {
		path       string
		capability *sourceCapability
		err        error
	}{
		{s.GlobalPath, s.global, s.globalErr},
		{s.ProjectPath, s.project, s.projectErr},
	} {
		if item.path == "" {
			continue
		}
		if item.err != nil {
			return "", fmt.Errorf("instructions: secure source parent %s: %w", item.path, item.err)
		}
		if item.capability == nil {
			// The parent was absent when Source was constructed. Never retry via
			// the pathname: a later symlink could redirect the read.
			continue
		}
		data, err := item.capability.parentReadBounded()
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return "", fmt.Errorf("instructions: read %s: %w", item.path, err)
		}
		if len(merged) > MaxSourceTotalBytes-len(data) {
			return "", fmt.Errorf("instructions: merged source exceeds maximum size of %d bytes", MaxSourceTotalBytes)
		}
		merged = append(merged, data...)
	}
	return string(merged), nil
}

func (p *sourceCapability) parentReadBounded() ([]byte, error) {
	info, err := p.parent.Lstat(p.name)
	if err != nil {
		return nil, err
	}
	if info.mode&os.ModeSymlink != 0 {
		return nil, fmt.Errorf("%s is a symlink", p.name)
	}
	if !info.mode.IsRegular() {
		return nil, fmt.Errorf("%s is not a regular file", p.name)
	}
	if info.size > MaxSourceFileBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", p.name, MaxSourceFileBytes)
	}
	file, err := p.parent.OpenRead(p.name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	return readBoundedFile(file, p.name)
}

func readBoundedFile(file *os.File, name string) ([]byte, error) {
	data, err := io.ReadAll(io.LimitReader(file, MaxSourceFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > MaxSourceFileBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", name, MaxSourceFileBytes)
	}
	return data, nil
}
