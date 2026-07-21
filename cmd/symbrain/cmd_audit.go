package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdAudit(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("audit", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	fmt.Fprintln(stdout, "symbrain audit: not yet implemented (planned for milestone v0.1.0-m3)")
	return exitcodes.ExitOK
}
