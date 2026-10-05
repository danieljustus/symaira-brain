//! Native memory help contracts.

/// The shipped `symbrain memory` help, as printed when no subcommand is given.
pub(super) const MEMORY_USAGE: &str = r"symbrain memory — embedded memory store operations

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
";

/// The shipped `symbrain memory list` help (`list --help`, also stdout with exit 0).
pub(super) const MEMORY_LIST_USAGE: &str = r"symbrain memory list — list stored memories

Usage:
  symbrain memory list [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum memories to return (default 100, max 1000).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
";

/// The shipped `symbrain memory sync` help. The Go source builds one line of it
/// by concatenating the token environment-variable constant; this is the
/// rendered text.
pub(super) const MEMORY_SYNC_USAGE: &str = r"symbrain memory sync — synchronize the embedded memory store with a remote

Usage:
  symbrain memory sync --remote <url> [flags]

Flags:
  --remote <url>          Base URL of the remote memory server (required).
                          Must use https, except http://localhost or
                          127.0.0.1 for local development.
  --pull                  Only pull remote changes into the local database.
  --push                  Only push local changes to the remote server.
                          (With neither flag, both directions run.)
  --token <token>         Bearer token for the remote API. May come from
                          $SYMBRAIN_MEMORY_SYNC_TOKEN instead; never pass it on the command line in
                          shared shells.
  --encrypted-relay       Exchange client-side AES-256-GCM encrypted blobs
                          through the remote /api/sync/relay endpoint, so the
                          relay never sees plaintext memory content.
  --relay-passphrase <p>  Passphrase for --encrypted-relay. May come from
                          $SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE instead. Both peers must share it.
  --allow-insecure-http   Override the https requirement for non-loopback
                          remotes. Bearer tokens will be sent in the clear;
                          a WARNING is printed. Use only for testing.
  --db <path>             Database path override (default: the standard
                          memory database under the XDG data directory).
  --timeout <duration>    Per-run HTTP timeout (default 60s).

The local database and its per-remote sync cursors are reused in place;
no export or import step is needed.
";

/// The line the shipped `memory sync` prints before its help when `--remote` is
/// missing.
pub(super) const MEMORY_SYNC_REMOTE_REQUIRED: &str = "symbrain memory sync: --remote is required\n";

/// Hand-written usage errors. The shipped implementation prints these itself
/// rather than through the flag package, so they are portable byte for byte.
pub(super) const MEMORY_SEARCH_USAGE_ERROR: &str =
    "usage: symbrain memory search <query> [--scope <scope>] [--limit <N>] [--db <path>]\n";
pub(super) const MEMORY_SET_USAGE_ERROR: &str =
    "usage: symbrain memory set <content> --kind <kind> [--scope <scope>] [flags]\n";
pub(super) const MEMORY_DELETE_USAGE_ERROR: &str =
    "usage: symbrain memory delete <id> [--db <path>]\n";

pub(super) const MEMORY_SEARCH_USAGE: &str =
    "symbrain memory search — search memories by semantic relevance

Usage:
  symbrain memory search <query> [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum results to return (default 5).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
";

pub(super) const QUERY_LOG_USAGE: &str =
    "symbrain memory query-log — inspect the memory retrieval log

Usage:
  symbrain memory query-log [flags]

Flags:
  --limit, -l <N>      Maximum recent entries to return (default 50, max 1000).
  --actor <name>       Filter recent entries by actor.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
";

pub(super) const MEMORY_RULES_USAGE: &str = r"symbrain memory rules — list procedural rules

Usage:
  symbrain memory rules [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
";

pub(super) const MEMORY_SET_USAGE: &str = r"symbrain memory set — store a memory in the embedded store

Usage:
  symbrain memory set <content> --kind <kind> [flags]

Flags:
  --kind, -k <kind>    Semantic kind: user, feedback, project, reference (required).
  --scope, -s <scope>  Scope: global (default), project, agent, user, or session.
  --author <name>      Author recorded on the memory (default cli:symbrain).
  --metadata <json>    JSON object of metadata key/value pairs.
  --entities <names>   Comma-separated entity names to link.
  --staged             Store as a candidate, excluded from retrieval until promoted.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
";

pub(super) const MEMORY_DELETE_USAGE: &str = r"symbrain memory delete — remove a memory from the embedded store

Usage:
  symbrain memory delete <id> [flags]

Flags:
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
";
