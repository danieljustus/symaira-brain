// Package spawn governs how stdio MCP servers are launched.
//
// Discovery finds MCP servers and the policy layer governs the calls made to
// them, but the launch itself — the executable path, argv, and the
// environment (where API keys travel) — is a separate attack surface. This
// package gates launches with an allowlist: only servers whose absolute
// command path and argv prefix match a configured [config.SpawnEntry] may be
// spawned.
//
// The allowlist is deny by default: an empty allowlist permits nothing, and
// a server whose command is not an absolute path can never match an entry.
// HTTP servers are reached by URL and have no executable to allowlist; they
// are not gated here.
package spawn

import (
	"path/filepath"
	"slices"

	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/discovery"
)

// Allowlist is a compiled set of spawn entries with deny-by-default
// semantics. The zero value is an empty allowlist that permits nothing.
type Allowlist struct {
	entries []config.SpawnEntry
}

// NewAllowlist compiles entries into an Allowlist.
func NewAllowlist(entries []config.SpawnEntry) *Allowlist {
	return &Allowlist{entries: entries}
}

// Len returns the number of allowlist entries.
func (a *Allowlist) Len() int { return len(a.entries) }

// Allows reports whether the server may be spawned: its absolute command
// path must match an entry's path exactly, and the entry's argv prefix must
// be a prefix of the server's arguments. An entry without an argv prefix
// matches any arguments. Non-stdio servers (HTTP) are not spawned and are
// always allowed here.
func (a *Allowlist) Allows(s discovery.Server) bool {
	if s.Transport != discovery.TransportStdio {
		return true
	}
	if !filepath.IsAbs(s.Command) {
		return false
	}
	cmd := filepath.Clean(s.Command)
	for _, e := range a.entries {
		if !filepath.IsAbs(e.Path) || filepath.Clean(e.Path) != cmd {
			continue
		}
		if len(e.ArgvPrefix) > len(s.Args) {
			continue
		}
		if slices.Equal(e.ArgvPrefix, s.Args[:len(e.ArgvPrefix)]) {
			return true
		}
	}
	return false
}
