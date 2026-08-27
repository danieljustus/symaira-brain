// cmd_guard — absorbed symguard command set under `symbrain guard`.
//
// The standalone `symguard` binary is retired (ADR 0001, D6): its command
// implementations moved into this module (guard/cmd/symguard/<verb>/) and are
// invoked in-process. `decide`, `scan`, `doctor` and `grants` behave exactly
// as the standalone commands did, including exit codes and the stdin/stdout
// wire format of `decide`.

package main

import (
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/decide"
	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/doctor"
	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/grants"
	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/scan"
	guardversion "github.com/danieljustus/symaira-brain/guard/cmd/symguard/version"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// cmdGuard dispatches to the absorbed symguard command implementations.
// Flag parsing is delegated wholesale to the subcommands — everything after
// `guard` is opaque, matching the standalone binary's behavior.
func cmdGuard(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) < 1 {
		printGuardUsage(stdout)
		return exitcodes.ExitNoInput
	}

	switch args[0] {
	case "decide":
		return exitcodes.ExitCode(decide.Run(args[1:], os.Stdin, stdout, nil))
	case "doctor":
		return exitcodes.ExitCode(doctor.Run(stdout))
	case "grants":
		grants.Run(args[1:], stdout)
		return exitcodes.ExitOK
	case "scan":
		return exitcodes.ExitCode(scan.Run(args[1:], stdout, stderr))
	case "version":
		guardversion.Run(args[1:], stdout)
		return exitcodes.ExitOK
	case "help", "--help", "-h":
		printGuardUsage(stdout)
		return exitcodes.ExitOK
	default:
		fmt.Fprintf(stderr, "symbrain guard: unknown command %q\n\n", args[0])
		printGuardUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func printGuardUsage(w io.Writer) {
	fmt.Fprintln(w, `symguard — local-first security gateway (absorbed into symbrain)

Usage:
  symbrain guard <command> [flags]

Commands:
  version   Print version and build info
  doctor    Check system health and configuration
  decide    Read a JSON decision request from stdin, write the decision to stdout
  grants    List and revoke standing grants
  scan      Discover MCP servers across supported AI clients
  help      Show this help message

Run 'symbrain guard <command> --help' for details on a specific command.`)
}
