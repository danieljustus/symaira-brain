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
		return cmdDoctorWithFormat(rest, stdout, stderr, format)
	case "profile":
		return cmdProfileWithFormat(rest, stdout, stderr, format)
	case "config":
		return cmdConfig(rest, stdout, stderr)
	case "harness":
		return cmdHarnessWithFormat(rest, stdout, stderr, format)
	case "usage":
		return cmdUsageWithFormat(rest, stdout, stderr, format)
	case "mcp":
		return cmdMcp(rest, stdout, stderr)
	case "serve":
		// Deprecated alias for `symbrain mcp` (kept for one minor
		// release). The notice goes to stderr only: stdout is the MCP
		// JSON-RPC transport and must stay clean (Zero Stdio Pollution).
		fmt.Fprintln(stderr, "symbrain: 'serve' is deprecated; use 'symbrain mcp' instead (serve will be removed in a future release)")
		return cmdMcp(rest, stdout, stderr)
	case "install":
		return cmdInstall(rest, stdout, stderr)
	case "uninstall":
		return cmdUninstall(rest, stdout, stderr)
	case "setup":
		return cmdSetup(rest, stdout, stderr)
	case "sync":
		return cmdSyncWithFormat(rest, stdout, stderr, format)
	case "memory":
		return cmdMemoryWithFormat(rest, stdout, stderr, format)
	case "activity":
		return cmdActivityWithFormat(rest, stdout, stderr, format)
	case "audit":
		return cmdAuditWithFormat(rest, stdout, stderr, format)
	case "version":
		return cmdVersionWithFormat(rest, stdout, stderr, format)
	case "vault":
		return cmdPassthrough(cmd, rest, stderr)
	case "guard":
		return cmdGuard(rest, stdout, stderr)
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
	return extractFormat(args)
}

// peekCommand scans args skipping known global output flags and their values
// to find the command name. This avoids calling Extract (which can error on
// dangling or unsupported values) for commands that don't support them.
func peekCommand(args []string) string {
	for i := 0; i < len(args); i++ {
		switch {
		case args[i] == "--json" || args[i] == "-json":
			continue
		case args[i] == "--output" || args[i] == "-output":
			i++ // skip the value
			continue
		case strings.HasPrefix(args[i], "--output=") || strings.HasPrefix(args[i], "-output="):
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
	case "version", "sync", "memory", "activity", "profile", "harness", "audit", "doctor", "usage":
		return true
	default:
		return false
	}
}

func printUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain — portable agent-context layer for AI harnesses

Usage:
  symbrain <command> [flags]

Global output flags (version, sync, memory, activity, profile, harness, audit, usage, and doctor):
  --output table|json  Output format (default: table)
  --json               Shorthand for --output json

Commands:
  init        Create XDG directories, default config, and example profiles
  doctor      Check environment, config, profiles, and child binaries
  setup       Download and install pinned core binaries to ~/.symaira/bin
  profile     Manage profiles (list, show, add, remove)
  config      Inspect and edit the global config (path, get, set)
  harness     Inspect registered AI harnesses and their MCP servers
  usage       AI subscription/token usage per provider
  mcp         Run the MCP gateway over stdio for a profile (serve is a deprecated alias)
  install     Register symbrain with a harness
  uninstall   Remove symbrain from a harness
  sync        Sync instructions and skills to harnesses
  memory      Operate the embedded memory store (sync with a remote)
  activity    Read bounded activity summaries with explicit profile access
  audit       Inspect the audit log
  vault       Passthrough to symvault
  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)

  version     Print version information
  help        Show this help message

Run 'symbrain <command> --help' for details on a specific command.
`)
}
