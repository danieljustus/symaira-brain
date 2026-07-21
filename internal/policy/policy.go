package policy

import (
	"fmt"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

// Verdict classifies one live tool relative to a server's resolved
// exposure policy.
type Verdict string

const (
	// Exposed means the tool passes the active policy (preset/allow minus
	// deny) and the child actually reports it.
	Exposed Verdict = "exposed"
	// Hidden means the tool is known to this package's reference tool list
	// for the server but is excluded by the active mode, an explicit
	// tools_allow, or tools_deny.
	Hidden Verdict = "hidden"
	// Unknown means the tool is not part of any versioned preset this
	// package maintains for the server — an upstream tool this package has
	// never been told about. Unknown tools are always excluded from
	// Exposed (default-deny), regardless of mode.
	Unknown Verdict = "unknown"
)

// Report is the structured result of evaluating one server's exposure
// policy against a live (or preset-derived) tool list. Exposed, Hidden,
// and Unknown partition the input tool list — every tool passed to
// Evaluate appears in exactly one of the three, sorted for deterministic
// output. It is a plain typed value intended for reuse by `doctor` and
// audit logging (see internal/policy doc comment).
type Report struct {
	Server  string `json:"server"`
	Enabled bool   `json:"enabled"`
	// Mode is only meaningful for vault and memory; empty for skills.
	Mode    string   `json:"mode,omitempty"`
	Exposed []string `json:"exposed"`
	Hidden  []string `json:"hidden"`
	Unknown []string `json:"unknown"`
}

// Verdict returns the classification for tool. A tool absent from the
// evaluated live list (i.e. never seen by Evaluate) is reported as Unknown,
// the same default-deny bucket as an unrecognized live tool.
func (r *Report) Verdict(tool string) Verdict {
	for _, t := range r.Exposed {
		if t == tool {
			return Exposed
		}
	}
	for _, t := range r.Hidden {
		if t == tool {
			return Hidden
		}
	}
	return Unknown
}

// vaultTools are the versioned, in-repo-maintained tool lists for each
// vault mode preset. New upstream symvault tools do not appear here
// automatically — they must be added deliberately, which is exactly the
// default-deny-for-unrecognized-tools guarantee this package provides.
var vaultTools = struct {
	RequestOnly []string
	Full        []string
}{
	RequestOnly: []string{"request_credential", "generate_password", "health"},
	Full: []string{
		"find_entries",
		"generate_password",
		"get_entry",
		"get_entry_metadata",
		"health",
		"request_credential",
		"set_entry_field",
		"symaira_audit_self",
		"symaira_search",
		"symaira_whoami",
	},
}

// memoryTools are the versioned, in-repo-maintained tool lists for each
// memory mode preset.
var memoryTools = struct {
	ReadOnly  []string
	ReadWrite []string
}{
	ReadOnly: []string{
		"entity_list",
		"entity_resolve",
		"graph_neighbors",
		"memory_get",
		"memory_list",
		"memory_search",
	},
	ReadWrite: []string{
		"entity_list",
		"entity_relate",
		"entity_resolve",
		"graph_neighbors",
		"memory_get",
		"memory_list",
		"memory_search",
		"memory_set",
	},
}

// presetForMode returns the versioned tool list for alias's mode, and
// whether the (alias, mode) combination is recognized at all. An empty,
// non-nil-ok result (vault "off") is a valid preset that exposes nothing.
func presetForMode(alias, mode string) ([]string, bool) {
	switch alias {
	case profile.ServerVault:
		switch mode {
		case profile.VaultModeRequestOnly:
			return vaultTools.RequestOnly, true
		case profile.VaultModeFull:
			return vaultTools.Full, true
		case profile.VaultModeOff:
			return nil, true
		}
	case profile.ServerMemory:
		switch mode {
		case profile.MemoryModeReadOnly:
			return memoryTools.ReadOnly, true
		case profile.MemoryModeReadWrite:
			return memoryTools.ReadWrite, true
		}
	}
	return nil, false
}

// universeFor returns the maximal versioned tool list this package knows
// about for alias (vault's "full" preset, memory's "read_write" preset).
// Skills has no bounded universe — see Evaluate — and returns nil.
func universeFor(alias string) []string {
	switch alias {
	case profile.ServerVault:
		return vaultTools.Full
	case profile.ServerMemory:
		return memoryTools.ReadWrite
	default:
		return nil
	}
}

// KnownTools returns a copy of the maximal versioned tool list this
// package knows about for alias. Returns nil for skills, which has no
// bounded preset universe (see Evaluate).
func KnownTools(alias string) []string {
	return append([]string(nil), universeFor(alias)...)
}

// PresetTools returns a copy of the versioned tool list for alias's mode
// preset (e.g. PresetTools("vault", "request_only")). It returns an error
// for an unrecognized alias/mode combination. Skills has no modes and
// always errors — see Evaluate for its allow/deny-only policy instead.
func PresetTools(alias, mode string) ([]string, error) {
	preset, ok := presetForMode(alias, mode)
	if !ok {
		return nil, fmt.Errorf("policy: no preset for server %q mode %q", alias, mode)
	}
	return append([]string(nil), preset...), nil
}

// Evaluate computes the exposure Report for one server alias
// ("vault"|"memory"|"skills") given its resolved profile.ServerConfig and
// the child's live tool list (typically from tools/list). Pass the
// package's own KnownTools(alias) as liveTools to preview policy before a
// live connection exists — see EvaluatePreset for vault/memory's ready-made
// version of that.
//
// Resolution order, matching the profile schema's documented rules
// (tools_allow/tools_deny override mode presets; deny always wins):
//
//  1. If the server is disabled, or vault's mode is "off", nothing is
//     exposed — allow/deny are not consulted.
//  2. Otherwise the base set is tools_allow if non-empty (an explicit
//     allow list replaces the mode preset entirely), else the active
//     mode's preset (vault/memory) or the full live list (skills, which
//     has no modes and is always-full-when-enabled).
//  3. tools_deny is then subtracted from the base set, unconditionally.
//  4. The result is intersected with liveTools: only tools the child
//     actually reports can be Exposed.
//
// Every tool in liveTools ends up in exactly one of Exposed, Hidden
// (known to this package but not exposed), or Unknown (never in any
// preset this package maintains for the server — the regression this
// package specifically guards against: an upstream tool absent from the
// preset list is never exposed just because it exists).
func Evaluate(alias string, cfg profile.ServerConfig, liveTools []string) (*Report, error) {
	switch alias {
	case profile.ServerVault, profile.ServerMemory, profile.ServerSkills:
	default:
		return nil, fmt.Errorf("policy: unknown server alias %q", alias)
	}

	report := &Report{Server: alias, Enabled: cfg.Enabled, Mode: cfg.Mode}

	off := !cfg.Enabled || (alias == profile.ServerVault && cfg.Mode == profile.VaultModeOff)
	if off {
		hidden, unknown := classify(alias, liveTools, nil)
		report.Exposed = []string{}
		report.Hidden = hidden
		report.Unknown = unknown
		return report, nil
	}

	var base map[string]bool
	switch alias {
	case profile.ServerSkills:
		// No modes, no bounded universe: default is everything the child
		// reports, narrowed only by an explicit tools_allow.
		if len(cfg.ToolsAllow) > 0 {
			base = toSet(cfg.ToolsAllow)
		} else {
			base = toSet(liveTools)
		}
	default:
		preset, ok := presetForMode(alias, cfg.Mode)
		if !ok {
			return nil, fmt.Errorf("policy: unknown mode %q for server %q", cfg.Mode, alias)
		}
		if len(cfg.ToolsAllow) > 0 {
			base = toSet(cfg.ToolsAllow)
		} else {
			base = toSet(preset)
		}
	}

	deny := toSet(cfg.ToolsDeny)

	var exposed []string
	exposedSet := make(map[string]bool, len(liveTools))
	for _, tool := range liveTools {
		if base[tool] && !deny[tool] {
			exposed = append(exposed, tool)
			exposedSet[tool] = true
		}
	}
	sort.Strings(exposed)

	hidden, unknown := classify(alias, liveTools, exposedSet)
	report.Exposed = nonNil(exposed)
	report.Hidden = hidden
	report.Unknown = unknown
	return report, nil
}

// EvaluatePreset resolves the exposure Report for a mode-based server
// (vault or memory) using this package's own versioned reference tool list
// as a stand-in "live" catalog. This is what `symbrain profile show` uses
// to describe effective policy before any broker/child connection exists
// (see AGENTS.md "Standalone-First": symbrain never assumes a child binary
// is present). Skills has no modes/preset universe, so it is not supported
// here — callers should evaluate skills directly with whatever live or
// configured tool list is appropriate for the context.
func EvaluatePreset(alias string, cfg profile.ServerConfig) (*Report, error) {
	if alias != profile.ServerVault && alias != profile.ServerMemory {
		return nil, fmt.Errorf(
			"policy: EvaluatePreset only supports %q and %q (skills has no mode preset)",
			profile.ServerVault, profile.ServerMemory)
	}
	return Evaluate(alias, cfg, universeFor(alias))
}

// classify partitions liveTools (skipping anything already in exposed)
// into hidden (known to alias's reference universe) and unknown (not part
// of any preset this package maintains for alias). Skills has no bounded
// universe, so everything not exposed there is Hidden, never Unknown.
func classify(alias string, liveTools []string, exposed map[string]bool) (hidden, unknown []string) {
	universe := universeFor(alias)
	bounded := alias == profile.ServerVault || alias == profile.ServerMemory
	known := toSet(universe)

	for _, tool := range liveTools {
		if exposed[tool] {
			continue
		}
		if !bounded || known[tool] {
			hidden = append(hidden, tool)
		} else {
			unknown = append(unknown, tool)
		}
	}
	sort.Strings(hidden)
	sort.Strings(unknown)
	return nonNil(hidden), nonNil(unknown)
}

func toSet(items []string) map[string]bool {
	set := make(map[string]bool, len(items))
	for _, it := range items {
		set[it] = true
	}
	return set
}

func nonNil(s []string) []string {
	if s == nil {
		return []string{}
	}
	return s
}
