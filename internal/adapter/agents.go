package adapter

import "github.com/danieljustus/symaira-brain/internal/instructions"

// AgentsTarget is the adapter for AGENTS.md-aware harnesses (Codex, Cursor,
// OpenCode, and others).  It writes the full canonical content directly.
var AgentsTarget = Target{
	Name:     "agents",
	Filename: "AGENTS.md",
	Render: func(content, _ string) string {
		return instructions.Render("", content)
	},
}
