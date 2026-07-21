package main

import (
	"flag"
	"fmt"
	"io"
	"runtime"

	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/versionkit"
)

// versionSchema is the versionkit schema_version for symbrain's `version
// --json` output. Bump it whenever that JSON shape changes incompatibly —
// see corekit/versionkit for the GUI<->core handshake this drives.
const versionSchema = 1

func cmdVersion(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("version", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	info := versionkit.New("symbrain", version, versionSchema)

	if *jsonOut {
		if err := info.Write(stdout); err != nil {
			fmt.Fprintf(stderr, "symbrain version: %v\n", err)
			return exitcodes.ExitGeneric
		}
		return exitcodes.ExitOK
	}

	fmt.Fprintln(stdout, info.String())
	fmt.Fprintf(stdout, "  go      %s\n", runtime.Version())
	fmt.Fprintf(stdout, "  os/arch %s/%s\n", runtime.GOOS, runtime.GOARCH)
	return exitcodes.ExitOK
}
