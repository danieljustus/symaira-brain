package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"time"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/usage"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdUsage(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain usage: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdUsageWithFormat(args, stdout, stderr, format)
}

func cmdUsageWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("usage", flag.ContinueOnError)
	fs.SetOutput(stderr)
	fs.Usage = func() { printUsageUsage(stderr) }
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain usage: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	report := usage.BuildReport(ctx, usage.AllProviders(nil))
	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			printUsageReport(w, report)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain usage: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printUsageUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain usage — AI subscription/token usage per provider

Usage:
  symbrain usage

The global --output table|json flag (or --json) selects the output format.

Providers: Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal,
OpenCode, OpenRouter, Antigravity. Credential resolution: an explicit env
var per provider, whose value may be a symvault://<path> URI resolved
through the secret store; providers with a native CLI credential file
fall back to it read-only when the env var is unset. See each provider's
doc comment in internal/usage for the macOS-Keychain / local-database
strategies not ported from the Swift original.
`)
}

func printUsageReport(w io.Writer, report usage.Report) {
	for _, p := range report.Providers {
		fmt.Fprintf(w, "%s\t%s\t", p.ID, p.DisplayName)
		switch {
		case !p.Configured:
			fmt.Fprintf(w, "not configured\t%s\n", p.AuthStatus.Detail)
		case p.Error != "":
			fmt.Fprintf(w, "error\t%s\n", p.Error)
		case p.Snapshot != nil:
			fmt.Fprintf(w, "ok\tsource=%s\n", p.Snapshot.Source)
			for _, m := range p.Snapshot.Meters {
				fmt.Fprintf(w, "  %s\t%s\t%s\n", m.Label, meterValue(m), m.Unit)
			}
		default:
			fmt.Fprintln(w, "unknown")
		}
	}
}

func meterValue(m usage.UsageMeter) string {
	used := "?"
	if m.Used != nil {
		used = *m.Used
	}
	if m.Limit != nil {
		return used + "/" + *m.Limit
	}
	return used
}
