package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdAudit(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain audit: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdAuditWithFormat(args, stdout, stderr, format)
}

func cmdAuditWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("audit", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	sub, rest := "", fs.Args()
	if len(rest) > 0 {
		sub, rest = rest[0], rest[1:]
	}

	switch sub {
	case "tail":
		return cmdAuditTailWithFormat(rest, stdout, stderr, format)
	case "":
		fmt.Fprintln(stderr, "symbrain audit: subcommand required (tail)")
		return exitcodes.ExitNoInput
	default:
		fmt.Fprintf(stderr, "symbrain audit: unknown subcommand %q\n", sub)
		return exitcodes.ExitNoInput
	}
}

func cmdAuditTail(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain audit tail: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdAuditTailWithFormat(args, stdout, stderr, format)
}

func cmdAuditTailWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("audit tail", flag.ContinueOnError)
	profile := fs.String("profile", "", "filter by profile name")
	n := fs.Int("n", 20, "number of entries to show")
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	if *jsonOut || format == output.FormatJSON {
		return cmdAuditTailJSON(stdout, stderr, *profile, *n)
	}

	if err := audit.Tail(stdout, *profile, *n); err != nil {
		fmt.Fprintf(stderr, "symbrain audit tail: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

// cmdAuditTailJSON reads the last n entries from the audit log and emits
// them as a JSON array to stdout.
func cmdAuditTailJSON(stdout, stderr io.Writer, profile string, n int) exitcodes.ExitCode {
	entries, err := audit.TailEntries(profile, n)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain audit tail --json: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if err := json.NewEncoder(stdout).Encode(entries); err != nil {
		fmt.Fprintf(stderr, "symbrain audit tail --json: encode: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}
