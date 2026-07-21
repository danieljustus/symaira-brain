package adapter

import (
	"fmt"

	"github.com/danieljustus/symaira-brain/internal/instructions"
)

// ClaudeTarget is the adapter for Claude Code.  CLAUDE.md carries a thin
// pointer referencing AGENTS.md plus the managed block for project-specific
// additions, mirroring the pattern used across the Symaira workspace.
var ClaudeTarget = Target{
	Name:     "claude",
	Filename: "CLAUDE.md",
	Render: func(content, projectDir string) string {
		pointer := fmt.Sprintf(
			"<!-- symbrain:managed-pointer -->\n"+
				"This file is managed by `symbrain sync`. "+
				"Canonical instructions live in AGENTS.md.\n"+
				"Edit AGENTS.md for global changes; use the managed block below for project-specific additions.\n\n"+
				"%s\n",
			instructions.Render("", content),
		)
		return pointer
	},
}
