package adapter

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
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
	// harness-specific format. Existing bytes are supplied so user content
	// outside the managed block can be preserved verbatim.
	Render func(existing, content, projectDir string) string
}

var targetsByCapability = map[harness.InstructionAdapter]Target{
	harness.InstructionAdapterAgents:      AgentsTarget,
	harness.InstructionAdapterClaude:      ClaudeTarget,
	harness.InstructionAdapterCursor:      CursorTarget,
	harness.InstructionAdapterAntigravity: AntigravityTarget,
}

// TargetsForHarnesses derives the sync adapter map from the harness registry.
func TargetsForHarnesses() map[string]Target {
	targets := make(map[string]Target)
	for _, h := range harness.All {
		if h.InstructionAdapter == harness.InstructionAdapterNone {
			continue
		}
		if target, ok := targetsByCapability[h.InstructionAdapter]; ok {
			targets[string(h.Name)] = target
		}
	}
	return targets
}

// Sync writes the adapter's output file into a validated path beneath
// projectDir. The retained parent capability spans read, render, and write.
func Sync(t Target, content, projectDir string) (string, bool, error) {
	relativeTarget := t.Filename
	if t.Dir != "" {
		relativeTarget = filepath.Join(t.Dir, t.Filename)
	}
	path := filepath.Join(projectDir, relativeTarget)

	file, err := instructions.OpenAtomicFile(projectDir, relativeTarget, false)
	if err != nil && !os.IsNotExist(err) {
		return "", false, fmt.Errorf("adapter %s: open %s: %w", t.Name, path, err)
	}
	existed := false
	var existingBytes []byte
	if err == nil {
		existingBytes, err = file.ReadBounded()
		existed = err == nil
		if err != nil && !os.IsNotExist(err) {
			_ = file.Close()
			return "", false, fmt.Errorf("adapter %s: read %s: %w", t.Name, path, err)
		}
	}
	if file != nil {
		defer file.Close()
	}
	existing := string(existingBytes)

	rendered := t.Render(existing, content, projectDir)
	if rendered == existing {
		return path, false, nil
	}

	if file == nil {
		file, err = instructions.OpenAtomicFile(projectDir, relativeTarget, true)
		if err != nil {
			return "", false, fmt.Errorf("adapter %s: open %s for write: %w", t.Name, path, err)
		}
		defer file.Close()
	}
	if err := file.Write([]byte(rendered)); err != nil {
		return "", false, fmt.Errorf("adapter %s: write %s: %w", t.Name, path, err)
	}

	return path, !existed, nil
}
