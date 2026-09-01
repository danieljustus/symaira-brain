package adapter

import (
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
)

// AntigravityTarget is the adapter for Antigravity (the app and its `agy`
// CLI, which replaced the Gemini CLI). GEMINI.md follows the same
// thin-pointer pattern as CLAUDE.md: a pointer to the canonical content
// plus the managed block for project-specific additions.
//
// The filename stays GEMINI.md deliberately. Antigravity reads GEMINI.md
// and AGENTS.md; writing AGENTS.md here would put two harnesses (`agents`
// and `antigravity`) on the same file, and every existing checkout already
// carries a managed GEMINI.md.
var AntigravityTarget = Target{
	Name:     string(harness.Antigravity),
	Filename: "GEMINI.md",
	Render: func(content, _ string) string {
		return instructions.Render("", content)
	},
}
