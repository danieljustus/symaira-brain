// Foreign-server exposure model (ADR 0001, D3).
//
// For a server outside the four cores no in-repo tool universe exists and
// none can be maintained, so the preset model cannot apply: stretching it
// would produce a permanent Unknown verdict for every tool — a rule that
// denies everything is not a policy, it is an outage. Instead the foreign
// model is a filter: evaluation order is enabled → access class (read /
// write) → tools_allow / tools_deny, with deny winning. Read/write
// classification precedence: an explicit tools_read/tools_write entry in the
// profile wins; otherwise readOnlyHint: true counts as reading; otherwise
// the tool counts as writing.
//
// The semantic difference to the cores is stated in the README: for the
// cores symbrain is a default-deny gatekeeper, for foreign servers it is a
// filter. An unknown tool of a foreign server under access = "write" with
// an empty tools_allow is exposed.
package policy

import (
	"fmt"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

// ForeignTool is the minimal tool shape the foreign exposure model needs:
// the tool name and, optionally, the upstream server's readOnlyHint
// annotation. The broker/catalog types are intentionally not imported here
// (catalog imports policy).
type ForeignTool struct {
	Name         string
	ReadOnlyHint *bool
}

// Exposure sources, recorded in the audit log so an exposure decision can be
// explained after the fact.
const (
	ExposureSourceToolsRead    = "tools_read"
	ExposureSourceToolsWrite   = "tools_write"
	ExposureSourceReadOnlyHint = "read_only_hint"
	ExposureSourceDefaultWrite = "default_write"
)

// ToolExposure records the access classification of one foreign-server tool
// and what it was derived from.
type ToolExposure struct {
	Class  string `json:"class"`  // "read" | "write"
	Source string `json:"source"` // ExposureSource*
}

// EvaluateForeign computes the exposure Report for a foreign server. Every
// live tool lands in Exposed or Hidden — there is no Unknown bucket, because
// for a foreign server an unmaintainable reference list does not exist; the
// filter model exposes an unknown tool unless the access class or an
// allow/deny entry removes it. The per-tool access classification and its
// source are recorded on the Report (Exposures) for the audit log.
func EvaluateForeign(alias string, cfg profile.ServerConfig, tools []ForeignTool) (*Report, error) {
	access := cfg.Access
	if access == "" {
		access = profile.ForeignAccessWrite
	}
	switch access {
	case profile.ForeignAccessRead, profile.ForeignAccessWrite:
	default:
		return nil, fmt.Errorf("policy: invalid access class %q for foreign server %q", access, alias)
	}

	report := &Report{Server: alias, Enabled: cfg.Enabled}
	report.Exposures = make(map[string]ToolExposure, len(tools))

	if !cfg.Enabled {
		hidden := make([]string, 0, len(tools))
		for _, t := range tools {
			hidden = append(hidden, t.Name)
			report.Exposures[t.Name] = classifyForeignTool(cfg, t)
		}
		sort.Strings(hidden)
		report.Hidden = hidden
		report.Exposed = []string{}
		return report, nil
	}

	base := make(map[string]bool, len(tools))
	if len(cfg.ToolsAllow) > 0 {
		for _, t := range cfg.ToolsAllow {
			base[t] = true
		}
	} else {
		for _, t := range tools {
			base[t.Name] = true
		}
	}
	deny := toSet(cfg.ToolsDeny)

	var exposed []string
	hiddenSet := make(map[string]bool, len(tools))
	for _, t := range tools {
		exposure := classifyForeignTool(cfg, t)
		report.Exposures[t.Name] = exposure

		if access == profile.ForeignAccessRead && exposure.Class != profile.ForeignAccessRead {
			hiddenSet[t.Name] = true
			continue
		}
		if !base[t.Name] || deny[t.Name] {
			hiddenSet[t.Name] = true
			continue
		}
		exposed = append(exposed, t.Name)
	}

	hidden := make([]string, 0, len(hiddenSet))
	for name := range hiddenSet {
		hidden = append(hidden, name)
	}
	sort.Strings(exposed)
	sort.Strings(hidden)
	report.Exposed = nonNil(exposed)
	report.Hidden = nonNil(hidden)
	return report, nil
}

// classifyForeignTool resolves one tool's read/write class by precedence:
// explicit tools_read / tools_write entry wins, then the upstream
// readOnlyHint, else default-write.
func classifyForeignTool(cfg profile.ServerConfig, t ForeignTool) ToolExposure {
	for _, name := range cfg.ToolsRead {
		if name == t.Name {
			return ToolExposure{Class: profile.ForeignAccessRead, Source: ExposureSourceToolsRead}
		}
	}
	for _, name := range cfg.ToolsWrite {
		if name == t.Name {
			return ToolExposure{Class: profile.ForeignAccessWrite, Source: ExposureSourceToolsWrite}
		}
	}
	if t.ReadOnlyHint != nil && *t.ReadOnlyHint {
		return ToolExposure{Class: profile.ForeignAccessRead, Source: ExposureSourceReadOnlyHint}
	}
	return ToolExposure{Class: profile.ForeignAccessWrite, Source: ExposureSourceDefaultWrite}
}
