// Package main is the CLI entrypoint for symbrain, the portable
// agent-context layer that multiplexes the Symaira state cores (vault,
// memory, skills) for AI harnesses.
package main

import (
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/logkit"
)

// version is set at build time via ldflags:
//
//	go build -ldflags "-X main.version=v0.1.0"
//
// Default value is "dev" for untagged builds. The Makefile injects the
// result of `git describe` automatically.
var version = "dev"

func main() {
	logkit.InitDefault("symbrain")
	os.Exit(int(run(os.Args[1:], os.Stdout, os.Stderr)))
}

// run dispatches the given args to the matching subcommand and returns the
// process exit code. Output goes to stdout, diagnostics to stderr.
func run(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, normalized, err := globalOutput(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if len(normalized) < 1 {
		printUsage(stdout)
		return exitcodes.ExitNoInput
	}

	cmd, rest := normalized[0], normalized[1:]

	switch cmd {
	case "init":
		return cmdInit(rest, stdout, stderr)
	case "doctor":
		return cmdDoctor(rest, stdout, stderr)
	case "profile":
		return cmdProfileWithFormat(rest, stdout, stderr, format)
	case "serve":
		return cmdServe(rest, stdout, stderr)
	case "install":
		return cmdInstall(rest, stdout, stderr)
	case "uninstall":
		return cmdUninstall(rest, stdout, stderr)
	case "setup":
		return cmdSetup(rest, stdout, stderr)
	case "sync":
		return cmdSyncWithFormat(rest, stdout, stderr, format)
	case "audit":
		return cmdAudit(rest, stdout, stderr)
	case "version":
		return cmdVersionWithFormat(rest, stdout, stderr, format)
	case "vault", "memory", "skills":
		return cmdPassthrough(cmd, rest, stderr)
	case "help", "--help", "-h":
		printUsage(stdout)
		return exitcodes.ExitOK
	default:
		fmt.Fprintf(stderr, "symbrain: unknown command %q\n\n", cmd)
		printUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func globalOutput(args []string) (output.Format, []string, error) {
	// Identify the command first, so Extract is only called for commands
	// that actually support global output flags. For all other commands,
	// args pass through untouched — their own flag parsers handle --json,
	// --output etc. locally.
	cmd := peekCommand(args)
	if !isOutputCommand(cmd) {
		return output.FormatTable, args, nil
	}
	format, normalized, err := output.Extract(args)
	return format, normalized, err
}

// peekCommand scans args skipping known global output flags and their values
// to find the command name. This avoids calling Extract (which can error on
// dangling or unsupported values) for commands that don't support them.
func peekCommand(args []string) string {
	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "--json":
			continue
		case args[i] == "--output":
			i++ // skip the value
			continue
		case strings.HasPrefix(args[i], "--output="):
			continue
		default:
			if !strings.HasPrefix(args[i], "-") {
				return args[i]
			}
		}
	}
	return ""
}

func isOutputCommand(command string) bool {
	switch command {
	case "version", "sync", "profile", "audit", "doctor":
		return true
	default:
		return false
	}
}

func printUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain — portable agent-context layer for AI harnesses

Usage:
  symbrain <command> [flags]

Global output flags (version, sync, profile, audit, and doctor):
  --output table|json  Output format (default: table)
  --json               Shorthand for --output json

Commands:
  init        Create XDG directories, default config, and example profiles
  doctor      Check environment, config, profiles, and child binaries
  setup       Download and install pinned core binaries to ~/.symaira/bin
  profile     Manage profiles (list, show, add, remove)
  serve       Run the MCP gateway over stdio for a profile
  install     Register symbrain with a harness
  uninstall   Remove symbrain from a harness
  sync        Sync instructions and skills to harnesses
  audit       Inspect the audit log
  vault       Passthrough to symvault
  memory      Passthrough to symmemory
  skills      Passthrough to symskills
  version     Print version information
  help        Show this help message

Run 'symbrain <command> --help' for details on a specific command.
`)
}
