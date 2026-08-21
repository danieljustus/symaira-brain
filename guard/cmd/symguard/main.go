// Package main is the CLI entrypoint for symguard.
//
// symguard is a local-first security gateway for AI agents,
// MCP servers, and Symaira toolchains.
//
// Each subcommand lives in its own package under cmd/symguard/<verb>/
// and exposes a Run([]string, io.Writer) int entry point.
package main

import (
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-guard/cmd/symguard/decide"
	"github.com/danieljustus/symaira-guard/cmd/symguard/doctor"
	"github.com/danieljustus/symaira-guard/cmd/symguard/grants"
	"github.com/danieljustus/symaira-guard/cmd/symguard/scan"
	"github.com/danieljustus/symaira-guard/cmd/symguard/version"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout))
}

// run executes the CLI command with the given args and writes output to w.
// It returns an exit code: 0 for success, 1 for usage errors.
func run(args []string, w io.Writer) int {
	if len(args) < 1 {
		printUsage(w)
		return 1
	}

	code := 0
	switch args[0] {
	case "version":
		version.Run(args[1:], w)
	case "doctor":
		code = doctor.Run(w)
	case "decide":
		code = decide.Run(args[1:], os.Stdin, w, nil)
	case "grants":
		grants.Run(args[1:], w)
	case "scan":
		code = scan.Run(args[1:], w, os.Stderr)
	case "help", "--help", "-h":
		printUsage(w)
	default:
		fmt.Fprintf(w, "unknown command: %s\n\n", args[0])
		printUsage(w)
		return 1
	}

	// Best-effort update check after interactive commands. Machine-facing
	// commands (decide, scan) skip it: a decision or scan run must never
	// wait on an unreachable GitHub.
	switch args[0] {
	case "decide", "scan":
	default:
		version.CheckLatest()
	}

	return code
}

func printUsage(w io.Writer) {
	fmt.Fprintln(w, `symguard — local-first security gateway for AI agents

Usage:
  symguard <command> [flags]

Commands:
  version   Print version and build info
  doctor    Check system health and configuration
  decide    Read a JSON decision request from stdin, write the decision to stdout
  grants    List and revoke standing grants
  scan      Discover MCP servers across supported AI clients
  help      Show this help message

Run 'symguard <command> --help' for details on a specific command.`)
}
