package harness

import (
	"path/filepath"
	"strings"
)

// Entry is the MCP server entry symbrain writes into (and removes from) a
// harness's config file.
type Entry struct {
	Command string
	Args    []string
}

// NewEntry builds the standard symbrain MCP entry that binds a harness
// connection to profile: {"command": "symbrain", "args": ["serve",
// "--profile", profile]} (or the TOML-equivalent table for codex).
func NewEntry(profile string) Entry {
	return Entry{
		Command: ServerName,
		Args:    []string{"serve", "--profile", profile},
	}
}

// IsSymbrain reports whether the entry's command resolves to the symbrain
// binary, whether it was recorded as the bare name "symbrain" or as a
// resolved/absolute path ending in it. uninstall uses this to remove only
// symbrain's own entry and never touch unrelated MCP servers sharing the
// same config file.
func (e Entry) IsSymbrain() bool {
	return e.Command != "" && filepath.Base(e.Command) == ServerName
}

// Profile extracts the --profile value bound in the entry's args, if any.
// It accepts both "--profile <name>" and "--profile=<name>" forms.
func (e Entry) Profile() (string, bool) {
	for i, a := range e.Args {
		if a == "--profile" {
			if i+1 < len(e.Args) {
				return e.Args[i+1], true
			}
			return "", false
		}
		if v, ok := strings.CutPrefix(a, "--profile="); ok {
			return v, true
		}
	}
	return "", false
}
