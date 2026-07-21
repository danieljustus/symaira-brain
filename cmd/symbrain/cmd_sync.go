package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/sync"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdSync(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("sync", flag.ContinueOnError)
	projectDir := fs.String("project", "", "project directory (default: current directory)")
	dryRun := fs.Bool("dry-run", false, "show what would be written without making changes")
	jsonOutput := fs.Bool("json", false, "output results as JSON")
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

	if *jsonOutput {
		if err := sync.FormatSummaryJSON(stdout, statuses, skillsResults); err != nil {
			fmt.Fprintf(stderr, "symbrain sync: format JSON: %v\n", err)
			return exitcodes.ExitGeneric
		}
	} else {
		sync.FormatSummary(stdout, statuses, skillsResults)
	}

	return exitcodes.ExitOK
}
