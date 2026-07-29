package main

import (
	"flag"
	"fmt"
	"io"
	"runtime"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/versionkit"
)

// versionSchema is the versionkit schema_version for symbrain's `version
// --json` output. Bump it whenever that JSON shape changes incompatibly —
// see corekit/versionkit for the GUI<->core handshake this drives.
const versionSchema = 1

func cmdVersion(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain version: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdVersionWithFormat(args, stdout, stderr, format)
}

func cmdVersionWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("version", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain version: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	info := versionkit.New("symbrain", version, versionSchema)
	rows := output.Rows{
		JSON: info,
		Table: func(w io.Writer) error {
			if _, err := fmt.Fprintln(w, info.String()); err != nil {
				return err
			}
			if _, err := fmt.Fprintf(w, "  go      %s\n", runtime.Version()); err != nil {
				return err
			}
			_, err := fmt.Fprintf(w, "  os/arch %s/%s\n", runtime.GOOS, runtime.GOARCH)
			return err
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain version: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}
