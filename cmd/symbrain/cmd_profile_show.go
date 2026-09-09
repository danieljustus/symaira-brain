package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

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
