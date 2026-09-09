package adapter

import (
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
)

// CursorTarget is the adapter for Cursor's rules format.
// .cursor/rules/symbrain.mdc carries a YAML frontmatter header followed by
// the managed block of instructions content.
var CursorTarget = Target{
	Name:     string(harness.Cursor),
	Filename: "symbrain.mdc",
	Dir:      ".cursor/rules",
	Render: func(existing, content, _ string) string {
		if existing != "" {
			return instructions.Render(existing, content)
		}
		header := "---\n" +
			"description: Symaira brain managed instructions\n" +
			"globs: **/*\n" +
			"alwaysApply: true\n" +
			"---\n\n"
		return header + instructions.Render("", content)
	},
}
