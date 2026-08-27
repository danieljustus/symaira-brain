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
// connection to profile: {"command": "symbrain", "args": ["mcp",
// "--profile", profile]} (or the TOML-equivalent table for codex).
func NewEntry(profile string) Entry {
	return Entry{
		Command: ServerName,
		Args:    []string{"mcp", "--profile", profile},
	}
}

// SupersededCoreNames lists the standalone core binaries symbrain now serves
// in-process (repo consolidation step 4): memory and skills. Vault stays
// deliberately a separate process and is never in this set.
var SupersededCoreNames = []string{"symmemory", "symskills"}

// SupersededCore reports whether the entry's command resolves to a
// superseded core binary that symbrain now serves in-process. Matching is
// by command basename, so both the bare-name form ("symmemory") and a
// managed-runtime/absolute path form ("/opt/homebrew/bin/symmemory") are
// covered; an entry the user renamed or repointed is never matched. Returns
// the matched binary name. install uses this to migrate a harness away from
// entries that would duplicate (and lose to, on tool-name collisions) the
// gateway's own tools.
func (e Entry) SupersededCore() (string, bool) {
	base := filepath.Base(e.Command)
	for _, name := range SupersededCoreNames {
		if base == name {
			return name, true
		}
	}
	return "", false
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
