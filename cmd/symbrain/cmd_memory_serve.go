package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// defaultMemoryServePort matches the memory server's own default bind
// address (see NewServer's bindAddr field in internal/memory/mcp/server.go).
const defaultMemoryServePort = 8787

// cmdMemoryServe implements `symbrain memory serve`: it exposes the
// embedded memory store's HTTP API (already fully built and tested behind
// StartHTTPServer/HTTPHandler) as a standalone process, giving `symbrain
// memory sync --remote <url>` a first-party peer to talk to. The server is
// always bound to 127.0.0.1 — StartHTTPServer does not accept a non-loopback
// bind address, and this command does not attempt to change that.
func cmdMemoryServe(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("memory serve", flag.ContinueOnError)
	port := fs.Int("port", defaultMemoryServePort, "TCP port to listen on (always bound to 127.0.0.1)")
	dbPath := fs.String("db", "", "database path override (default: the standard memory database)")
	fs.SetOutput(stderr)
	fs.Usage = func() { printMemoryServeUsage(stderr) }
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain memory serve: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}
	if *port <= 0 || *port > 65535 {
		fmt.Fprintf(stderr, "symbrain memory serve: --port must be between 1 and 65535\n")
		return exitcodes.ExitNoInput
	}

	srv, closeDB, err := buildMemoryHTTPServer(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory serve: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer closeDB()

	fmt.Fprintf(stdout, "symbrain memory serve: listening on http://127.0.0.1:%d\n", *port)
	if err := srv.StartHTTPServer(*port); err != nil {
		fmt.Fprintf(stderr, "symbrain memory serve: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

// buildMemoryHTTPServer opens the embedded memory runtime (config + SQLite
// DB + JWT provider) and constructs its MCP server, the same way `symbrain
// mcp` builds the in-process memory core (see buildMemoryServer in
// cmd_serve.go). It is factored out separately from buildMemoryServer
// because `memory serve` has no brain profile to attribute requests to and
// runs standalone rather than embedded in the stdio gateway. The caller is
// responsible for invoking the returned cleanup func to close the database.
func buildMemoryHTTPServer(dbPath string) (*memorymcp.Server, func(), error) {
	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	if dbPath != "" {
		memcfg.Database.Path = dbPath
	}

	memdb, err := db.Open(memcfg)
	if err != nil {
		return nil, nil, fmt.Errorf("open memory database: %w", err)
	}

	memjwt, err := security.NewJWTProvider(memcfg, memdb)
	if err != nil {
		_ = memdb.Close()
		return nil, nil, fmt.Errorf("init memory JWT provider: %w", err)
	}

	srv := memorymcp.NewServer(memdb, memjwt, version, memcfg)
	return srv, func() { _ = memdb.Close() }, nil
}

func printMemoryServeUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory serve — run the memory HTTP API as a sync peer

Usage:
  symbrain memory serve [flags]

Flags:
  --port <n>   TCP port to listen on (default 8787). Always bound to
               127.0.0.1; never exposed on a non-loopback address.
  --db <path>  Database path override (default: the standard memory
               database under the XDG data directory).

This starts the same HTTP API 'symbrain memory sync --remote' talks to:
/api/sync/changes, /api/sync/apply, /api/sync/relay, plus /api/search,
/api/set, /api/list, /api/get, /api/delete, /api/rules, /api/stats and
/api/status. All routes except /api/status require a bearer token; see
'symbrain memory sync --help' for how tokens are supplied to the client
side of a sync. The process runs until interrupted (Ctrl-C/SIGTERM), at
which point it shuts down gracefully.
`)
}
