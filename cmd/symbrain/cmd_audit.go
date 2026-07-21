package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdAudit(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("audit", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	sub, rest := "", fs.Args()
	if len(rest) > 0 {
		sub, rest = rest[0], rest[1:]
	}

	switch sub {
	case "tail":
		return cmdAuditTail(rest, stdout, stderr)
	case "":
		fmt.Fprintln(stderr, "symbrain audit: subcommand required (tail)")
		return exitcodes.ExitNoInput
	default:
		fmt.Fprintf(stderr, "symbrain audit: unknown subcommand %q\n", sub)
		return exitcodes.ExitNoInput
	}
}

func cmdAuditTail(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("audit tail", flag.ContinueOnError)
	profile := fs.String("profile", "", "filter by profile name")
	n := fs.Int("n", 20, "number of entries to show")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	if err := audit.Tail(stdout, *profile, *n); err != nil {
		fmt.Fprintf(stderr, "symbrain audit tail: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}
