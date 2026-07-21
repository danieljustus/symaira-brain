// Package main is the CLI entrypoint for symbrain, the portable
// agent-context layer that multiplexes the Symaira state cores (vault,
// memory, skills) for AI harnesses.
package main

import (
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/logkit"
)

func main() {
	logkit.InitDefault("symbrain")
	os.Exit(int(run(os.Args[1:], os.Stdout, os.Stderr)))
}

// run dispatches the given args to the matching subcommand and returns the
// process exit code. Output goes to stdout, diagnostics to stderr.
func run(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) < 1 {
		printUsage(stdout)
		return exitcodes.ExitNoInput
	}

	cmd, rest := args[0], args[1:]

	switch cmd {
	case "init":
		return cmdInit(rest, stdout, stderr)
	case "doctor":
		return cmdDoctor(rest, stdout, stderr)
	case "profile":
		return cmdProfile(rest, stdout, stderr)
	case "serve":
		return cmdServe(rest, stdout, stderr)
	case "install":
		return cmdInstall(rest, stdout, stderr)
	case "uninstall":
		return cmdUninstall(rest, stdout, stderr)
	case "sync":
		return cmdSync(rest, stdout, stderr)
	case "audit":
		return cmdAudit(rest, stdout, stderr)
	case "version":
		return cmdVersion(rest, stdout, stderr)
	case "help", "--help", "-h":
		printUsage(stdout)
		return exitcodes.ExitOK
	default:
		fmt.Fprintf(stderr, "symbrain: unknown command %q\n\n", cmd)
		printUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func printUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain — portable agent-context layer for AI harnesses

Usage:
  symbrain <command> [flags]

Commands:
  init        Create XDG directories, default config, and example profiles
  doctor      Check environment, config, profiles, and child binaries
  profile     Manage profiles (list, show, add, remove)
  serve       Run the MCP gateway over stdio for a profile
  install     Register symbrain with a harness
  uninstall   Remove symbrain from a harness
  sync        Sync instructions and skills to harnesses
  audit       Inspect the audit log
  version     Print version information
  help        Show this help message

Run 'symbrain <command> --help' for details on a specific command.
`)
}
