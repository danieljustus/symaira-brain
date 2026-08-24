package main

import (
	"flag"
	"fmt"
	"io"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdHarnessWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if len(args) == 0 || args[0] != "list" {
		printHarnessUsage(stderr)
		return exitcodes.ExitNoInput
	}

	fs := flag.NewFlagSet("harness list", flag.ContinueOnError)
	projectDir := fs.String("project", "", "project directory to inspect for project-local harness config")
	fs.SetOutput(stderr)
	if err := fs.Parse(args[1:]); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain harness list: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	report := harness.List(*projectDir)
	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			printHarnessInventory(w, report)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain harness list: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printHarnessUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain harness — inspect configured AI harnesses

Usage:
  symbrain harness list [--project DIR]

The global --output table|json flag (or --json) selects the output format.
`)
}

func printHarnessInventory(w io.Writer, report harness.Inventory) {
	for _, item := range report.Harnesses {
		fmt.Fprintf(w, "%s\t%s\n", item.Name, item.DisplayName)
		printHarnessConfig(w, "  global", item.Global)
		if item.Project != nil {
			printHarnessConfig(w, "  project", *item.Project)
		}
		fmt.Fprintln(w)
	}
}

func printHarnessConfig(w io.Writer, label string, config harness.ConfigInventory) {
	state := "missing"
	switch {
	case config.Error != "":
		state = "invalid"
	case config.Parsed:
		state = "parsed"
	case config.Exists:
		state = "unparsed"
	}
	fmt.Fprintf(w, "%s\t%s\t%s\tservers=%s\n", label, config.Path, state, joinHarnessServers(config.Servers))
	if config.Error != "" {
		fmt.Fprintf(w, "    error: %s\n", config.Error)
	}
}

func joinHarnessServers(servers []string) string {
	if len(servers) == 0 {
		return "(none)"
	}
	return strings.Join(servers, ",")
}
