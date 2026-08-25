package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/syncclient"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// memorySyncTokenEnv names the environment variable that supplies the remote
// API token when --token is not given. The value is never echoed anywhere.
const memorySyncTokenEnv = "SYMBRAIN_MEMORY_SYNC_TOKEN"

// memorySyncPassphraseEnv names the environment variable that supplies the
// encrypted-relay passphrase when --relay-passphrase is not given. The value
// is never echoed anywhere.
const memorySyncPassphraseEnv = "SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE"

// cmdMemorySync implements `symbrain memory sync`: bidirectional remote
// memory synchronization against any server speaking the memory sync API
// (/api/sync/changes + apply, or the encrypted relay). The local database at
// the standard XDG path is reused in place — sync cursors and the oplog are
// read/written on the existing DB, no export or import step.
func cmdMemorySync(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := output.Extract(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory sync: %v\n", err)
		return exitcodes.ExitNoInput
	}

	fs := flag.NewFlagSet("memory sync", flag.ContinueOnError)
	remote := fs.String("remote", "", "base URL of the remote memory server (required)")
	pull := fs.Bool("pull", false, "only pull remote changes (default when neither flag is set: both directions)")
	push := fs.Bool("push", false, "only push local changes (default when neither flag is set: both directions)")
	token := fs.String("token", "", "bearer token for the remote API (or $"+memorySyncTokenEnv+")")
	relay := fs.Bool("encrypted-relay", false, "exchange client-side AES-encrypted blobs through the relay endpoint")
	passphrase := fs.String("relay-passphrase", "", "passphrase for --encrypted-relay (or $"+memorySyncPassphraseEnv+")")
	dbPath := fs.String("db", "", "database path override (default: the standard memory database)")
	timeout := fs.Duration("timeout", 60*time.Second, "per-run HTTP timeout")
	fs.SetOutput(stderr)
	fs.Usage = func() { printMemorySyncUsage(stderr) }
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain memory sync: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}
	if *remote == "" {
		fmt.Fprintf(stderr, "symbrain memory sync: --remote is required\n")
		printMemorySyncUsage(stderr)
		return exitcodes.ExitNoInput
	}
	pullOnly, pushOnly := *pull, *push
	if !pullOnly && !pushOnly {
		pullOnly, pushOnly = true, true
	}
	tok := *token
	if tok == "" {
		tok = os.Getenv(memorySyncTokenEnv)
	}
	pass := *passphrase
	if pass == "" {
		pass = os.Getenv(memorySyncPassphraseEnv)
	}
	if *relay && pass == "" {
		fmt.Fprintf(stderr, "symbrain memory sync: --encrypted-relay requires --relay-passphrase (or $%s)\n", memorySyncPassphraseEnv)
		return exitcodes.ExitNoInput
	}

	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	if *dbPath != "" {
		memcfg.Database.Path = *dbPath
	}
	database, err := db.Open(memcfg)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory sync: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	ctx, cancel := context.WithTimeout(context.Background(), *timeout)
	defer cancel()

	result, err := syncclient.Run(ctx, syncclient.Options{
		Remote:         *remote,
		Token:          tok,
		Pull:           pullOnly,
		Push:           pushOnly,
		EncryptedRelay: *relay,
		Passphrase:     pass,
		DB:             database,
		Timeout:        *timeout,
	})
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory sync: %v\n", err)
		return exitcodes.ExitGeneric
	}

	rows := output.Rows{
		JSON: result,
		Table: func(w io.Writer) error {
			printMemorySyncTable(w, result)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory sync: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printMemorySyncTable(w io.Writer, r *syncclient.Result) {
	pad := func(label string, v any) {
		fmt.Fprintf(w, "%-18s %v\n", label+":", v)
	}
	pad("Remote", r.Remote)
	pad("Mode", r.Mode)
	pad("Encrypted relay", r.EncryptedRelay)
	if !r.Cursor.IsZero() {
		pad("Cursor", r.Cursor.UTC().Format(time.RFC3339))
	}
	if !r.ServerTime.IsZero() {
		pad("Server time", r.ServerTime.UTC().Format(time.RFC3339))
	}
	pad("Pulled memories", r.PulledMemories)
	pad("Pulled deletes", r.PulledDeletes)
	pad("Pushed memories", r.PushedMemories)
	pad("Pushed deletes", r.PushedDeletes)
	if r.EncryptedRelay {
		pad("Relay blobs fetched", r.RelayFetched)
		pad("Relay blobs stored", r.RelayStored)
	}
}

func printMemorySyncUsage(w io.Writer) {
	fmt.Fprintf(w, `symbrain memory sync — synchronize the embedded memory store with a remote

Usage:
  symbrain memory sync --remote <url> [flags]

Flags:
  --remote <url>          Base URL of the remote memory server (required).
  --pull                  Only pull remote changes into the local database.
  --push                  Only push local changes to the remote server.
                          (With neither flag, both directions run.)
  --token <token>         Bearer token for the remote API. May come from
                          $%s instead; never pass it on the command line in
                          shared shells.
  --encrypted-relay       Exchange client-side AES-256-GCM encrypted blobs
                          through the remote /api/sync/relay endpoint, so the
                          relay never sees plaintext memory content.
  --relay-passphrase <p>  Passphrase for --encrypted-relay. May come from
                          $%s instead. Both peers must share it.
  --db <path>             Database path override (default: the standard
                          memory database under the XDG data directory).
  --timeout <duration>    Per-run HTTP timeout (default 60s).

The local database and its per-remote sync cursors are reused in place;
no export or import step is needed.
`, memorySyncTokenEnv, memorySyncPassphraseEnv)
}
