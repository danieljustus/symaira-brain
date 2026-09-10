// Package install installs rendered skill folders into supported harness paths.
package install

import (
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

const markerFile = ".symskills.json"

// MarkerSchemaVersion is the .symskills.json marker format version this
// build understands and writes. docs/marker-protocol.md is the contract the
// macOS client is bound by; bump only on incompatible marker changes.
const MarkerSchemaVersion = 1

type Mode string

const (
	ModeSymlink Mode = "symlink"
	ModeCopy    Mode = "copy"
)

type RenderedSkill struct {
	Target render.Target `json:"target"`
	Name   string        `json:"name"`
	Path   string        `json:"path"`
}

type Options struct {
	HomeDir    string       `json:"home_dir"`
	ProjectDir string       `json:"project_dir"`
	Scope      render.Scope `json:"scope"`
	Mode       Mode         `json:"mode"`
	DryRun     bool         `json:"dry_run"`
	// Force adopts a destination that was not installed by symskills. The
	// existing directory is moved to a backup location instead of being
	// deleted, so a hand-written skill is never lost silently.
	Force bool `json:"force"`
	// AllowExecutable preserves executable bits on resource files instead
	// of stripping them (the default install policy).
	AllowExecutable bool `json:"allow_executable"`
	// BaseDir overrides the base snapshot root (defaults to
	// ~/.local/share/symskills/base). It is written on install and removed
	// on uninstall.
	BaseDir string `json:"base_dir,omitempty"`
	// Target identifies the harness a skill is installed into. It is used
	// by Diff to locate the persisted base snapshot.
	Target render.Target `json:"target,omitempty"`
	// EventsPath enables best-effort operation logging for direct install
	// callers. Empty disables logging; MCP callers may record richer events.
	EventsPath string `json:"events_path,omitempty"`
	// EventToolVersion stamps direct-operation events when EventsPath is set.
	EventToolVersion string `json:"event_tool_version,omitempty"`
	// EventActor identifies the caller in the operation log.
	EventActor string `json:"event_actor,omitempty"`
}

// ResourceModeChange describes an executable-bit change applied (or planned)
// to one resource file during install. Modes are octal strings like "0755".
type ResourceModeChange struct {
	Path string `json:"path"`
	From string `json:"from"`
	To   string `json:"to"`
}

type Result struct {
	Action string        `json:"action"`
	Target render.Target `json:"target"`
	Name   string        `json:"name"`
	Path   string        `json:"path"`
	Mode   Mode          `json:"mode"`
	// BackupPath is set when --force adopted an unmanaged destination and
	// moved it aside.
	BackupPath string `json:"backup_path,omitempty"`
	// ModeChanges lists every resource whose executable bit was stripped
	// (or would be stripped in a dry run). Empty when AllowExecutable is
	// set or the bundle has no executable resources.
	ModeChanges []ResourceModeChange `json:"mode_changes,omitempty"`
}

type Marker struct {
	SchemaVersion int           `json:"schema_version,omitempty"`
	ManagedBy     string        `json:"managed_by"`
	Target        render.Target `json:"target"`
	Name          string        `json:"name"`
	RenderedAt    string        `json:"rendered_at"`
	Mode          Mode          `json:"mode"`
	Installed     string        `json:"installed"`
	SourceHash    string        `json:"source_hash,omitempty"`
	// AllowExecutable records whether the install preserved executable
	// bits (--allow-executable or the manifest setting). Additive field;
	// sync replays it so a re-install never silently strips bits.
	AllowExecutable bool `json:"allow_executable,omitempty"`
}

type Change struct {
	Path   string `json:"path"`
	Status string `json:"status"`
	// Diff is a unified-style content diff (left = freshly rendered,
	// right = installed copy) for modified files. It is best-effort
	// enrichment: empty for added/removed files and for modified files
	// whose content cannot be diffed (binary, unreadable, or too large).
	// Additive field — consumers that read only path/status are unaffected.
	Diff string `json:"diff,omitempty"`
}
