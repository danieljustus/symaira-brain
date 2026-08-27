package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/harness"
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
	format, args, err := extractFormat(args)
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
		if !strings.HasPrefix(a, "-") || a == "-" || a == "--" {
			positionals = append(positionals, a)
			continue
		}
		flags = append(flags, a)
		name := strings.TrimPrefix(strings.TrimPrefix(a, "--"), "-")
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
	aliases := sortedServerAliases(p.Servers)
	sums := make([]serverSummary, 0, len(aliases))
	for _, alias := range aliases {
		cfg := p.Servers[alias]
		sums = append(sums, serverSummary{Server: alias, Enabled: cfg.Enabled, Mode: cfg.Mode})
	}
	return sums
}

// sortedServerAliases returns the profile's server aliases in deterministic
// order: the four core aliases in their canonical order (vault, memory,
// skills, usage), then any foreign servers alphabetically.
func sortedServerAliases(servers profile.Servers) []string {
	aliases := make([]string, 0, len(servers))
	coreOrder := []string{profile.ServerVault, profile.ServerMemory, profile.ServerSkills, profile.ServerUsage}
	for _, alias := range coreOrder {
		if _, ok := servers[alias]; ok {
			aliases = append(aliases, alias)
		}
	}
	var foreign []string
	for alias := range servers {
		if !profile.IsCoreAlias(alias) {
			foreign = append(foreign, alias)
		}
	}
	sort.Strings(foreign)
	return append(aliases, foreign...)
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
	Command         string         `json:"command,omitempty"`
	Args            []string       `json:"args,omitempty"`
	URL             string         `json:"url,omitempty"`
	EffectivePolicy *policy.Report `json:"effective_policy,omitempty"`
	// Note explains why EffectivePolicy is absent (skills has no mode
	// preset — see internal/policy.EvaluatePreset; foreign servers have
	// no preset at all).
	Note string `json:"note,omitempty"`
}

func buildProfileShowReport(p *profile.Profile) profileShowReport {
	aliases := sortedServerAliases(p.Servers)
	reports := make([]profileShowServerReport, 0, len(aliases))
	for _, alias := range aliases {
		reports = append(reports, buildServerShowReport(alias, p.Servers[alias]))
	}
	return profileShowReport{
		Name:        p.Name,
		Description: p.Description,
		Audit:       p.Audit,
		Warnings:    p.Warnings,
		Servers:     reports,
	}
}

func buildServerShowReport(alias string, cfg profile.ServerConfig) profileShowServerReport {
	r := profileShowServerReport{
		Server:     alias,
		Enabled:    cfg.Enabled,
		Mode:       cfg.Mode,
		ToolsAllow: cfg.ToolsAllow,
		ToolsDeny:  cfg.ToolsDeny,
		Command:    cfg.Command,
		Args:       cfg.Args,
		URL:        cfg.URL,
	}
	switch {
	case profile.IsCoreAlias(alias) && (alias == profile.ServerVault || alias == profile.ServerMemory):
		if report, err := policy.EvaluatePreset(alias, cfg); err == nil {
			r.EffectivePolicy = report
		} else {
			r.Note = err.Error()
		}
	case alias == profile.ServerSkills:
		r.Note = "skills has no mode preset; effective tools are always-full-when-enabled, " +
			"narrowed only by tools_allow/tools_deny, and require a live connection to enumerate"
	case alias == profile.ServerUsage:
		r.Note = "usage has no mode preset; the single tool get_ai_usage is exposed when enabled, " +
			"narrowed only by tools_allow/tools_deny"
	default:
		r.Note = "foreign server: no mode preset; exposure is read/write classified per profile"
	}
	return r
}

func cmdProfileShow(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile show: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdProfileShowWithFormat(args, stdout, stderr, format)
}

func cmdProfileShowWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("profile show", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
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
	if err := fs.Parse(normalizeFlags(args)); err != nil {
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
	args = reorderFlagsFirst(args, map[string]bool{"project": true})

	fs := flag.NewFlagSet("profile remove", flag.ContinueOnError)
	force := fs.Bool("force", false, "skip the confirmation prompt")
	projectDir := fs.String("project", "", "project directory; check project-local harness configs for bindings")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile remove <name> [--force] [--project <dir>]")
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

	bindings := harness.ProfileBindings(name, *projectDir)
	if len(bindings) > 0 && !*force {
		fmt.Fprintf(stderr, "symbrain profile remove: profile %q is still bound to harnesses:\n", name)
		for _, b := range bindings {
			fmt.Fprintf(stderr, "  - %s (%s)\n", b.Harness, b.Path)
		}
		fmt.Fprintln(stderr, "Refusing to remove. Use --force to override.")
		return exitcodes.ExitGeneric
	}

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
