// Package adapter contains one small module per supported harness (claude,
// codex, cursor, opencode, gemini) that writes instructions and MCP server
// configuration in that harness's own format.
package adapter

import (
	"fmt"
	"os"
	"path/filepath"
)

// Target describes where an adapter writes its output file and how to
// resolve the content for that harness.
type Target struct {
	// Name is a human-readable label for the harness (e.g. "agents", "claude").
	Name string
	// Filename is the output file basename (e.g. "AGENTS.md", "CLAUDE.md").
	Filename string
	// Dir is the subdirectory under projectDir where the file is written.
	// Empty means projectDir itself.
	Dir string
	// Render transforms the canonical instructions content into the
	// harness-specific format. It receives the full merged content from
	// instructions.Source and the path to the project directory (for
	// relative references).
	Render func(content, projectDir string) string
}

// Sync writes the adapter's output file into the resolved path under
// projectDir.  When the file already exists it is passed through Render
// which manages the block markers to preserve user content outside them.
// Returns (path, created, error) where created reports whether the file
// was newly created (true) or updated in place (false).
func Sync(t Target, content, projectDir string) (string, bool, error) {
	dir := projectDir
	if t.Dir != "" {
		dir = filepath.Join(projectDir, t.Dir)
	}

	path := filepath.Join(dir, t.Filename)
	existed := fileExists(path)

	var existing string
	if existed {
		data, err := os.ReadFile(path)
		if err != nil {
			return "", false, fmt.Errorf("adapter %s: read %s: %w", t.Name, path, err)
		}
		existing = string(data)
	}

	rendered := t.Render(content, projectDir)
	if rendered == existing {
		return path, false, nil
	}

	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return "", false, fmt.Errorf("adapter %s: mkdir %s: %w", t.Name, filepath.Dir(path), err)
	}
	if err := os.WriteFile(path, []byte(rendered), 0o600); err != nil {
		return "", false, fmt.Errorf("adapter %s: write %s: %w", t.Name, path, err)
	}

	return path, !existed, nil
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
