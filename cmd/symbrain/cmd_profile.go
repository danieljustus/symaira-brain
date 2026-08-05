package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/output"
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
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdProfileWithFormat(args, stdout, stderr, format)
}

func cmdProfileWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if len(args) < 1 {
		printProfileUsage(stderr)
		return exitcodes.ExitNoInput
	}

	sub, rest := args[0], args[1:]
	switch sub {
	case "list":
		return cmdProfileListWithFormat(rest, stdout, stderr, format)
	case "show":
		return cmdProfileShowWithFormat(rest, stdout, stderr, format)
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

// reorderFlagsFirst moves recognized long ("--flag") flags — and, for the
// names listed in valueFlags, their following value — to the front of args,
// leaving positional arguments after them in their original relative order.
// It is used by profile add/remove for their command-local flags; output
// formatting is handled globally by internal/output.
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
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile list: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdProfileListWithFormat(args, stdout, stderr, format)
}

func cmdProfileListWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("profile list", flag.ContinueOnError)
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

	rows := output.Rows{
		JSON: entries,
		Table: func(w io.Writer) error {
			printProfileListHuman(w, entries)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain profile list: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
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
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile show: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdProfileShowWithFormat(args, stdout, stderr, format)
}

func cmdProfileShowWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("profile show", flag.ContinueOnError)
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

	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			printProfileShowHuman(w, report)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain profile show: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
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
