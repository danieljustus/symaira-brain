package main

import (
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// cmdMemory dispatches the embedded memory core subcommands. Memory is an
// embedded core, so every subcommand is routed through the normal dispatcher
// here — deliberately NOT through cmdPassthrough; keep it out of
// passthroughMap.
func cmdMemory(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdMemoryWithFormat(args, stdout, stderr, format)
}

func cmdMemoryWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if len(args) == 0 {
		printMemoryUsage(stderr)
		return exitcodes.ExitNoInput
	}
	switch args[0] {
	case "-h", "--help":
		printMemoryUsage(stdout)
		return exitcodes.ExitOK
	case "sync":
		return cmdMemorySyncWithFormat(args[1:], stdout, stderr, format)
	default:
		fmt.Fprintf(stderr, "symbrain memory: unknown subcommand %q\n\n", args[0])
		printMemoryUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func printMemoryUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory — embedded memory store operations

Usage:
  symbrain memory <subcommand> [flags]

Subcommands:
  sync        Synchronize memories with a remote memory server

Run 'symbrain memory sync --help' for details on the sync command.
`)
}
