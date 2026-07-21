package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

// confirmReader is read for the `profile remove` confirmation prompt.
// Overridable in tests so they never block on real stdin.
var confirmReader io.Reader = os.Stdin

func cmdProfile(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) < 1 {
		printProfileUsage(stderr)
		return exitcodes.ExitNoInput
	}

	sub, rest := args[0], args[1:]
	switch sub {
	case "list":
		return cmdProfileList(rest, stdout, stderr)
	case "show":
		return cmdProfileShow(rest, stdout, stderr)
	case "add":
		return cmdProfileAdd(rest, stdout, stderr)
	case "remove":
		return cmdProfileRemove(rest, stdout, stderr)
	case "help", "--help", "-h":
		printProfileUsage(stdout)
		return exitcodes.ExitOK
	default:
		fmt.Fprintf(stderr, "symbrain profile: unknown subcommand %q\n\n", sub)
		printProfileUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func printProfileUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain profile — manage profiles

Usage:
  symbrain profile list [--json]
  symbrain profile show <name> [--json]
  symbrain profile add <name> [--from personal|restricted]
  symbrain profile remove <name> [--force]

Flags may be written before or after the profile name.
`)
}

// reorderFlagsFirst moves recognized long ("--flag") flags — and, for the
// names listed in valueFlags, their following value — to the front of
// args, leaving positional arguments after them in their original
// relative order. The stdlib flag package stops parsing at the first
// non-flag token, so without this a user could only write
// `profile show --json myname`, never `profile show myname --json`, even
// though this command's own usage text shows the name first.
func reorderFlagsFirst(args []string, valueFlags map[string]bool) []string {
	var flags, positionals []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		if !strings.HasPrefix(a, "--") {
			positionals = append(positionals, a)
			continue
		}
		flags = append(flags, a)
		name := strings.TrimPrefix(a, "--")
		if valueFlags[name] && i+1 < len(args) {
			i++
			flags = append(flags, args[i])
		}
	}
	return append(flags, positionals...)
}

// ---- list ----

type profileListEntry struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	Error       string          `json:"error,omitempty"`
	Servers     []serverSummary `json:"servers,omitempty"`
}

type serverSummary struct {
	Server  string `json:"server"`
	Enabled bool   `json:"enabled"`
	Mode    string `json:"mode,omitempty"`
}

func serverSummaries(p *profile.Profile) []serverSummary {
	return []serverSummary{
		{Server: profile.ServerVault, Enabled: p.Servers.Vault.Enabled, Mode: p.Servers.Vault.Mode},
		{Server: profile.ServerMemory, Enabled: p.Servers.Memory.Enabled, Mode: p.Servers.Memory.Mode},
		{Server: profile.ServerSkills, Enabled: p.Servers.Skills.Enabled},
	}
}

func cmdProfileList(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, nil)

	fs := flag.NewFlagSet("profile list", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain profile list: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	results, err := profile.LoadAll()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile list: %s\n", exitcodes.FormatCLIError(err))
		return exitcodes.ExitCodeFromError(err)
	}

	entries := make([]profileListEntry, 0, len(results))
	for _, r := range results {
		if r.Err != nil {
			entries = append(entries, profileListEntry{Name: r.Name, Error: exitcodes.FormatCLIError(r.Err)})
			continue
		}
		entries = append(entries, profileListEntry{
			Name:        r.Profile.Name,
			Description: r.Profile.Description,
			Servers:     serverSummaries(r.Profile),
		})
	}

	if *jsonOut {
		if err := json.NewEncoder(stdout).Encode(entries); err != nil {
			fmt.Fprintf(stderr, "symbrain profile list: %v\n", err)
			return exitcodes.ExitGeneric
		}
		return exitcodes.ExitOK
	}

	printProfileListHuman(stdout, entries)
	return exitcodes.ExitOK
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

// ---- show ----

type profileShowReport struct {
	Name        string                    `json:"name"`
	Description string                    `json:"description"`
	Audit       profile.AuditConfig       `json:"audit"`
	Warnings    []string                  `json:"warnings,omitempty"`
	Servers     []profileShowServerReport `json:"servers"`
}

type profileShowServerReport struct {
	Server          string         `json:"server"`
	Enabled         bool           `json:"enabled"`
	Mode            string         `json:"mode,omitempty"`
	ToolsAllow      []string       `json:"tools_allow,omitempty"`
	ToolsDeny       []string       `json:"tools_deny,omitempty"`
	EffectivePolicy *policy.Report `json:"effective_policy,omitempty"`
	// Note explains why EffectivePolicy is absent (skills has no mode
	// preset — see internal/policy.EvaluatePreset).
	Note string `json:"note,omitempty"`
}

func buildProfileShowReport(p *profile.Profile) profileShowReport {
	return profileShowReport{
		Name:        p.Name,
		Description: p.Description,
		Audit:       p.Audit,
		Warnings:    p.Warnings,
		Servers: []profileShowServerReport{
			buildServerShowReport(profile.ServerVault, p.Servers.Vault),
			buildServerShowReport(profile.ServerMemory, p.Servers.Memory),
			buildServerShowReport(profile.ServerSkills, p.Servers.Skills),
		},
	}
}

func buildServerShowReport(alias string, cfg profile.ServerConfig) profileShowServerReport {
	r := profileShowServerReport{
		Server:     alias,
		Enabled:    cfg.Enabled,
		Mode:       cfg.Mode,
		ToolsAllow: cfg.ToolsAllow,
		ToolsDeny:  cfg.ToolsDeny,
	}
	switch alias {
	case profile.ServerVault, profile.ServerMemory:
		if report, err := policy.EvaluatePreset(alias, cfg); err == nil {
			r.EffectivePolicy = report
		} else {
			r.Note = err.Error()
		}
	case profile.ServerSkills:
		r.Note = "skills has no mode preset; effective tools are always-full-when-enabled, " +
			"narrowed only by tools_allow/tools_deny, and require a live connection to enumerate"
	}
	return r
}

func cmdProfileShow(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, nil)

	fs := flag.NewFlagSet("profile show", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile show <name> [--json]")
		return exitcodes.ExitNoInput
	}
	name := fs.Arg(0)

	p, err := profile.Load(name)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile show: %s\n", exitcodes.FormatCLIError(err))
		return exitcodes.ExitCodeFromError(err)
	}

	report := buildProfileShowReport(p)

	if *jsonOut {
		if err := json.NewEncoder(stdout).Encode(report); err != nil {
			fmt.Fprintf(stderr, "symbrain profile show: %v\n", err)
			return exitcodes.ExitGeneric
		}
		return exitcodes.ExitOK
	}

	printProfileShowHuman(stdout, report)
	return exitcodes.ExitOK
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

// ---- add ----

// profileNameFieldPattern matches the [profile] table's `name = "..."`
// line in the personal/restricted templates from cmd_init.go, capturing
// the whitespace between "name" and the value so the rewritten line keeps
// the template's own formatting.
var profileNameFieldPattern = regexp.MustCompile(`(?m)^name(\s*=\s*)"([^"]*)"`)

// renderProfileFromTemplate returns the personal or restricted profile
// template from cmd_init.go with its [profile] name field rewritten to
// name. It reuses those consts directly (same package) rather than
// forking a second copy of the TOML content.
func renderProfileFromTemplate(from, name string) (string, error) {
	var tmpl string
	switch from {
	case "personal":
		tmpl = personalProfileTOML
	case "restricted":
		tmpl = restrictedProfileTOML
	default:
		return "", fmt.Errorf("--from must be %q or %q, got %q", "personal", "restricted", from)
	}

	loc := profileNameFieldPattern.FindStringSubmatchIndex(tmpl)
	if loc == nil {
		return "", fmt.Errorf("internal error: %q template has no [profile] name field to rewrite", from)
	}
	ws := tmpl[loc[2]:loc[3]]
	return tmpl[:loc[0]] + "name" + ws + strconv.Quote(name) + tmpl[loc[1]:], nil
}

func cmdProfileAdd(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, map[string]bool{"from": true})

	fs := flag.NewFlagSet("profile add", flag.ContinueOnError)
	from := fs.String("from", "restricted", `template to create from: "personal" or "restricted"`)
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile add <name> [--from personal|restricted]")
		return exitcodes.ExitNoInput
	}
	name := fs.Arg(0)

	if err := profile.ValidateName(name); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if profile.Exists(name) {
		fmt.Fprintf(stderr, "symbrain profile add: profile %q already exists (%s)\n", name, profile.Path(name))
		return exitcodes.ExitNoInput
	}

	contents, err := renderProfileFromTemplate(*from, name)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitNoInput
	}

	if err := os.MkdirAll(xdg.ProfilesDir(), 0o700); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if err := fsutil.AtomicWriteFile(profile.Path(name), []byte(contents), 0o600); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "created %s (from %s)\n", profile.Path(name), *from)
	return exitcodes.ExitOK
}

// ---- remove ----

func cmdProfileRemove(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, nil)

	fs := flag.NewFlagSet("profile remove", flag.ContinueOnError)
	force := fs.Bool("force", false, "skip the confirmation prompt")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile remove <name> [--force]")
		return exitcodes.ExitNoInput
	}
	name := fs.Arg(0)

	if err := profile.ValidateName(name); err != nil {
		fmt.Fprintf(stderr, "symbrain profile remove: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if !profile.Exists(name) {
		fmt.Fprintf(stderr, "symbrain profile remove: profile %q does not exist\n", name)
		return exitcodes.ExitNoInput
	}

	// TODO(#20, milestone m4): refuse removal if this profile is
	// referenced by an installed harness's config, once internal/harness
	// exists and tracks that binding.

	if !*force {
		fmt.Fprintf(stdout, "Remove profile %q (%s)? [y/N]: ", name, profile.Path(name))
		reader := bufio.NewReader(confirmReader)
		line, _ := reader.ReadString('\n')
		answer := strings.ToLower(strings.TrimSpace(line))
		if answer != "y" && answer != "yes" {
			fmt.Fprintln(stdout, "aborted")
			return exitcodes.ExitOK
		}
	}

	if err := os.Remove(profile.Path(name)); err != nil {
		fmt.Fprintf(stderr, "symbrain profile remove: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "removed %s\n", profile.Path(name))
	return exitcodes.ExitOK
}
