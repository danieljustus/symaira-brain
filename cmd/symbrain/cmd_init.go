package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdInit(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("init", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	fmt.Fprintln(stdout, "symbrain init: not yet implemented")
	return exitcodes.ExitOK
}
