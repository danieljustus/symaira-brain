package main

import (
	"flag"
	"fmt"
	"io"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

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
