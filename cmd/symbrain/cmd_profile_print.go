package main

import (
	"fmt"
	"io"
	"strings"
)

// Human-readable output helpers for `symbrain profile list`/`show`. They
// are split from cmd_profile.go to keep each file under the 400-line
// rule; there is no behavior difference.

func printProfileUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain profile — manage profiles

Usage:
  symbrain profile list
  symbrain profile show <name>
  symbrain profile add <name> [--from personal|restricted]
  symbrain profile remove <name> [--force]

Use the global --output table|json flag (or --json) for list and show.
Flags may be written before or after the profile name.
`)
}

func printProfileListHuman(w io.Writer, entries []profileListEntry) {
	if len(entries) == 0 {
		fmt.Fprintln(w, "no profiles found (run `symbrain init` for examples, or `symbrain profile add`)")
		return
	}
	for _, e := range entries {
		if e.Error != "" {
			fmt.Fprintf(w, "%s\t(error: %s)\n\n", e.Name, e.Error)
			continue
		}
		fmt.Fprintf(w, "%s\t%s\n", e.Name, e.Description)
		parts := make([]string, 0, len(e.Servers))
		for _, s := range e.Servers {
			state := "off"
			switch {
			case s.Enabled && s.Mode != "":
				state = s.Mode
			case s.Enabled:
				state = "on"
			}
			parts = append(parts, fmt.Sprintf("%s=%s", s.Server, state))
		}
		fmt.Fprintf(w, "  %s\n\n", strings.Join(parts, "  "))
	}
}

func printProfileShowHuman(w io.Writer, r profileShowReport) {
	fmt.Fprintf(w, "profile: %s\n", r.Name)
	if r.Description != "" {
		fmt.Fprintf(w, "description: %s\n", r.Description)
	}
	fmt.Fprintf(w, "audit: enabled=%t\n", r.Audit.Enabled)
	if len(r.Warnings) > 0 {
		fmt.Fprintln(w, "warnings:")
		for _, warning := range r.Warnings {
			fmt.Fprintf(w, "  - %s\n", warning)
		}
	}
	fmt.Fprintln(w)

	for _, s := range r.Servers {
		fmt.Fprintf(w, "%s: enabled=%t", s.Server, s.Enabled)
		if s.Mode != "" {
			fmt.Fprintf(w, " mode=%s", s.Mode)
		}
		fmt.Fprintln(w)
		if len(s.ToolsAllow) > 0 {
			fmt.Fprintf(w, "  tools_allow: %s\n", strings.Join(s.ToolsAllow, ", "))
		}
		if len(s.ToolsDeny) > 0 {
			fmt.Fprintf(w, "  tools_deny:  %s\n", strings.Join(s.ToolsDeny, ", "))
		}
		switch {
		case s.EffectivePolicy != nil:
			fmt.Fprintf(w, "  effective exposed: %s\n", joinOrNone(s.EffectivePolicy.Exposed))
			fmt.Fprintf(w, "  effective hidden:  %s\n", joinOrNone(s.EffectivePolicy.Hidden))
		case s.Note != "":
			fmt.Fprintf(w, "  note: %s\n", s.Note)
		}
		fmt.Fprintln(w)
	}
}

func joinOrNone(items []string) string {
	if len(items) == 0 {
		return "(none)"
	}
	return strings.Join(items, ", ")
}
