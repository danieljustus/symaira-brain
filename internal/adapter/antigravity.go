package adapter

import (
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
)

// AntigravityTarget is the adapter for Antigravity (the app and its `agy`
// CLI, which replaced the Gemini CLI). GEMINI.md follows the same
// managed-block pattern as AGENTS.md.
var AntigravityTarget = Target{
	Name:     string(harness.Antigravity),
	Filename: "GEMINI.md",
	Render: func(existing, content, _ string) string {
		return instructions.Render(existing, content)
	},
}
