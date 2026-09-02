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
	case "list":
		return cmdMemoryListWithFormat(args[1:], stdout, stderr, format)
	case "search":
		return cmdMemorySearchWithFormat(args[1:], stdout, stderr, format)
	case "set":
		return cmdMemorySetWithFormat(args[1:], stdout, stderr, format)
	case "delete":
		return cmdMemoryDeleteWithFormat(args[1:], stdout, stderr, format)
	case "rules":
		return cmdMemoryRulesWithFormat(args[1:], stdout, stderr, format)
	case "query-log":
		return cmdMemoryQueryLogWithFormat(args[1:], stdout, stderr, format)
	case "sync":
		return cmdMemorySyncWithFormat(args[1:], stdout, stderr, format)
	case "serve":
		return cmdMemoryServe(args[1:], stdout, stderr)
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
  list        List stored memories (optionally filtered by scope)
  search      Search memories by semantic relevance
  set         Store a memory (requires --kind)
  delete      Remove a memory by id
  rules       List procedural rules
  query-log   Inspect the memory retrieval log
  sync        Synchronize memories with a remote memory server
  serve       Run the memory HTTP API as a sync peer for 'memory sync --remote'

Use --output table|json (or --json) for the result format. Read commands
accept --scope/-s, --limit/-l, and --db; search takes one query argument.
Run 'symbrain memory <subcommand> --help' for details.
For remote synchronization, run 'symbrain memory sync --help'.
To act as the remote peer for another machine's sync, run 'symbrain memory serve --help'.
`)
}
