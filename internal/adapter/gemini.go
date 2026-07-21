package adapter

import "github.com/danieljustus/symaira-brain/internal/instructions"

// GeminiTarget is the adapter for Gemini CLI.  GEMINI.md follows the same
// thin-pointer pattern as CLAUDE.md: a pointer to the canonical content
// plus the managed block for project-specific additions.
var GeminiTarget = Target{
	Name:     "gemini",
	Filename: "GEMINI.md",
	Render: func(content, _ string) string {
		return instructions.Render("", content)
	},
}
