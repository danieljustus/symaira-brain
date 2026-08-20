package instructions

import (
	"os"
	"strings"
	"testing"
)

// TestSkillFileDrift prevents the tracked skills/symmemory/SKILL.md from
// drifting out of sync with the canonical instructions.md source embedded
// in the binary. When the source is updated, the skill file must also be
// regenerated to keep the copy command in README.md working correctly.
func TestSkillFileDrift(t *testing.T) {
	// Read SKILL.md from the repo root (test runs from internal/instructions/)
	skillFile := "../skills/symmemory/SKILL.md"
	skillData, err := os.ReadFile(skillFile)
	if err != nil {
		t.Fatalf("cannot read skills/symmemory/SKILL.md: %v", err)
	}

	// Strip YAML frontmatter (everything between the first and second `---` lines)
	body := string(skillData)
	if strings.HasPrefix(body, "---\n") {
		rest := body[4:]
		if idx := strings.Index(rest, "---\n"); idx >= 0 {
			body = rest[idx+4:]
		}
	}

	// Normalize trailing whitespace for comparison
	expected := strings.TrimSpace(instructionsText)
	got := strings.TrimSpace(body)

	if expected != got {
		t.Fatalf(
			"skills/symmemory/SKILL.md body differs from internal/instructions/instructions.md\n\n" +
				"Regenerate with:\n" +
				"  cat internal/instructions/instructions.md > skills/symmemory/SKILL.md\n" +
				"and re-add the YAML frontmatter to skills/symmemory/SKILL.md",
		)
	}
}
