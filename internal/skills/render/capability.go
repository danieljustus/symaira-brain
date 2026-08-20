package render

import (
	"fmt"
	"sort"
)

// Capability names form a closed vocabulary describing what a harness runtime
// offers a skill. A skill declares what it needs with `requires` in
// symskills.toml; rendering for a target that states it lacks a required
// capability is refused rather than producing an install that asserts a
// compatibility which does not exist.
//
// The vocabulary is deliberately about the harness runtime, not about the
// machine: anything a shell can do (running git, creating worktrees, calling
// an HTTP API) is not a capability here, because every harness that can run
// commands has it.
const (
	// CapSubagents: the harness can dispatch work to a child agent it
	// manages itself (Claude Code's Agent tool, Hermes' delegate_task).
	CapSubagents = "subagents"
	// CapBackgroundTasks: work can continue between turns and report back,
	// rather than only running to completion inside one turn.
	CapBackgroundTasks = "background_tasks"
	// CapMCP: the harness can connect to Model Context Protocol servers.
	CapMCP = "mcp"
	// CapSlashCommands: skills are invocable by an explicit user-typed
	// command, not only by model-side discovery.
	CapSlashCommands = "slash_commands"
	// CapHooks: the harness runs user-configured hooks around tool calls.
	CapHooks = "hooks"
	// CapScheduledTasks: the harness can run a skill on a schedule without
	// an interactive session.
	CapScheduledTasks = "scheduled_tasks"
)

// Capabilities is the full vocabulary, in the order reports print it.
var Capabilities = []string{
	CapSubagents,
	CapBackgroundTasks,
	CapMCP,
	CapSlashCommands,
	CapHooks,
	CapScheduledTasks,
}

// Capability support states. A capability a target has not declared is
// unknown, not absent: symskills does not observe harness runtimes, and
// treating silence as "unsupported" would refuse renders on a guess.
const (
	CapabilitySupported   = "supported"
	CapabilityUnsupported = "unsupported"
	CapabilityUnknown     = "unknown"
)

// IsCapability reports whether name belongs to the vocabulary.
func IsCapability(name string) bool {
	for _, cap := range Capabilities {
		if cap == name {
			return true
		}
	}
	return false
}

// CapabilityState returns the declared state of one capability for a target.
func CapabilityState(target Target, name string) string {
	spec, ok := LookupSpec(target)
	if !ok {
		return CapabilityUnknown
	}
	supported, declared := spec.Capabilities[name]
	switch {
	case !declared:
		return CapabilityUnknown
	case supported:
		return CapabilitySupported
	default:
		return CapabilityUnsupported
	}
}

// CapabilityStates returns the state of every vocabulary entry for a target.
func CapabilityStates(target Target) map[string]string {
	states := make(map[string]string, len(Capabilities))
	for _, name := range Capabilities {
		states[name] = CapabilityState(target, name)
	}
	return states
}

// DeclareCapabilities merges user-declared capabilities into the registry,
// overriding built-in declarations for the same target and capability. It is
// how a user records what their harness build actually does — the built-in
// registry declares only what is verifiable from public harness behaviour and
// leaves everything else unknown. An unknown capability name is an error, so a
// typo is reported instead of silently declaring nothing.
func DeclareCapabilities(declarations map[string]map[string]bool) error {
	names := make([]string, 0, len(declarations))
	for name := range declarations {
		names = append(names, name)
	}
	sort.Strings(names)

	for _, name := range names {
		target := Target(name)
		index := -1
		for i := range Targets {
			if Targets[i].Name == target {
				index = i
				break
			}
		}
		if index < 0 {
			return fmt.Errorf("capabilities: unknown target %q", name)
		}
		caps := make([]string, 0, len(declarations[name]))
		for cap := range declarations[name] {
			caps = append(caps, cap)
		}
		sort.Strings(caps)
		for _, cap := range caps {
			if !IsCapability(cap) {
				return fmt.Errorf("capabilities: target %q declares unknown capability %q", name, cap)
			}
			if Targets[index].Capabilities == nil {
				Targets[index].Capabilities = map[string]bool{}
			}
			Targets[index].Capabilities[cap] = declarations[name][cap]
		}
	}
	return nil
}

// CapabilityGap is one unmet requirement of a skill on one target.
type CapabilityGap struct {
	Capability string `json:"capability"`
	State      string `json:"state"`
}

// checkRequirements classifies a skill's declared requirements against a
// target. Unsupported capabilities refuse the render; unknown ones warn and
// render, because an undeclared capability is missing information rather than
// evidence of absence.
func checkRequirements(requires []string, target Target) (unsupported, unknown []CapabilityGap, err error) {
	for _, name := range requires {
		if !IsCapability(name) {
			return nil, nil, fmt.Errorf("skill requires unknown capability %q; known capabilities are %v", name, Capabilities)
		}
		switch state := CapabilityState(target, name); state {
		case CapabilityUnsupported:
			unsupported = append(unsupported, CapabilityGap{Capability: name, State: state})
		case CapabilityUnknown:
			unknown = append(unknown, CapabilityGap{Capability: name, State: state})
		}
	}
	return unsupported, unknown, nil
}
