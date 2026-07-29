package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/sync"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdSync(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain sync: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdSyncWithFormat(args, stdout, stderr, format)
}

func cmdSyncWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("sync", flag.ContinueOnError)
	projectDir := fs.String("project", "", "project directory (default: current directory)")
	dryRun := fs.Bool("dry-run", false, "show what would be written without making changes")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	harnessNames := fs.Args()
	if *projectDir == "" {
		*projectDir = "."
	}

	statuses, skillsResults, err := sync.Run(*projectDir, harnessNames, *dryRun, stderr)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain sync: %v\n", err)
		return exitcodes.ExitGeneric
	}

	rows := output.Rows{
		JSON: sync.Summary{Targets: statuses, Skills: skillsResults},
		Table: func(w io.Writer) error {
			sync.FormatSummary(w, statuses, skillsResults)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain sync: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}
