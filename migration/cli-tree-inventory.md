# Go Command Tree and Flags Inventory

This document provides an evidence-based inventory of the Go reference CLI command tree and flags for CLI-006.

## Source
Generated from: `cmd/symbrain/main.go` and `cmd/symbrain/cmd_*.go` files in the Go implementation.
Verified against: `target/go/symbrain-go` binary (built from pinned revision via `scripts/run-go-oracle.sh`).

## Top-Level Command Tree

```
symbrain — portable agent-context layer for AI harnesses

Usage:
  symbrain <command> [flags]

Global output flags (version, sync, memory, skills, activity, profile, harness, audit, usage, and doctor):
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
  memory      Operate the embedded memory store (list, search, set, delete, rules, query-log, sync, serve)
  skills      Operate the embedded skill library (list, status, targets, log, sync, doctor)
  activity    Read bounded activity summaries with explicit profile access
  audit       Inspect the audit log
  vault       Human credential management (create <path> and set <path.field> read single-line secrets from stdin; delete requires --yes)
  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)

  version     Print version information
  help        Show this help message

Vault approval passthrough:
  symbrain vault approval list [--output json]
  symbrain vault approval decide <request-id> --approve|--deny

Run 'symbrain <command> --help' for details on a specific command.
```

## Command Details

### init
- **Usage**: `symbrain init`
- **Flags**: None
- **Exit codes**:
  - Success: `0` (ExitOK)
  - No arguments: `100` (ExitNoInput) - shows usage
  - Unexpected arguments: `64` (ExitUSAGE) - shows "unexpected argument" error

### version
- **Usage**: `symbrain version [--json] [--output table|json] [--] [extra args]`
- **Flags**:
  - `--json`: Shorthand for `--output json`
  - `--output table|json`: Output format
  - `--`: Terminator (flags after treated as positional)
- **Behavior**:
  - No args: Prints human-readable version
  - `--json`: Prints JSON: `{"tool":"symbrain","version":"<ver>","schema_version":1}`
  - Unknown flag: Exit 64 (ExitUSAGE), prints "flag provided but not defined: -{flag}" + version usage
  - Missing flag value: Not applicable (boolean flags only)
  - Extra positional: Exit 64 (ExitUSAGE), prints "unexpected argument {arg}"
  - `--help`/`-h`: Prints version usage only, Exit 64 (ExitUSAGE)
- **Subcommands**: None

### help
- **Usage**: `symbrain help` or `symbrain --help` or `symbrain -h`
- **Flags**: None
- **Exit codes**: Success: `0` (ExitOK)
- **Behavior**: Prints full usage (as shown in top-level command tree)
- **Subcommands**: None

### config
- **Usage**: `symbrain config <subcommand> [flags]`
- **Subcommands**:
  - `path`: Print config file path
  - `get [key]`: Get config value (key optional, prints whole file if omitted)
  - `set <key> <value>`: Set config value
- **Subcommand: path**
  - **Usage**: `symbrain config path`
  - **Flags**: None
  - **Exit codes**:
    - Success: `0` (ExitOK) - prints path ending in "config.toml\\n"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
    - Unexpected flags: Exit 64 (ExitUSAGE) - "unexpected argument {flag}"
- **Subcommand: get**
  - **Usage**: `symbrain config get [key]`
  - **Flags**: None
  - **Exit codes**:
    - Success: `0` (ExitOK) - prints value or whole file
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
    - Unexpected flags: Exit 64 (ExitUSAGE) - "unexpected argument {flag}"
- **Subcommand: set**
  - **Usage**: `symbrain config set <key> <value>`
  - **Flags**:
    - `--preview`: Show change without writing (creates .bak)
    - `--no-backup`: Disable backup creation
  - **Exit codes**:
    - Missing `<key> <value>`: Exit 64 (ExitUSAGE) - "want exactly <key> <value>"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
    - Unexpected flags: Exit 64 (ExitUSAGE) - "unexpected argument {flag}"
- **Global flags**: `--output table|json`, `--json` (when used with subcommands that support it)

### doctor
- **Usage**: `symbrain doctor [flags]`
- **Flags**:
  - `-fix`: Repair missing/version-mismatched managed binaries
  - `-force-release`: With `-fix`: allow replacing brain-source build with pinned release
  - `-json`: Emit machine-readable JSON
  - `-vault-agent string`: Vault agent name for MCP handshake probe (default "claude-code")
- **Exit codes**:
  - Success: `0` (ExitOK)
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + doctor usage
  - Missing flag value for `-vault-agent`: Exit 64 (ExitUSAGE) - "flag needs an argument: -vault-agent" + doctor usage
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Note**: Returns Exit 1 when issues found (but not flag parsing errors)

### profile
- **Usage**: `symbrain profile <subcommand> [flags]`
- **Subcommands**:
  - `list`: List profiles
  - `show <name>`: Show profile details
  - `add <name> [--from personal|restricted]`: Add profile
  - `remove <name> [--force]`: Remove profile
- **Global flags**: `--output table|json`, `--json` (for list and show)
- **Subcommand: list**
  - **Usage**: `symbrain profile list`
  - **Flags**: None
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: show**
  - **Usage**: `symbrain profile show <name>`
  - **Flags**: None
  - **Exit codes**:
    - Missing `<name>`: Exit 64 (ExitUSAGE) - "unexpected argument"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: add**
  - **Usage**: `symbrain profile add <name> [--from personal|restricted]`
  - **Flags**: `--from string`: Template to create from ("personal" or "restricted", default "restricted")
  - **Exit codes**:
    - Missing `<name>`: Exit 64 (ExitUSAGE) - "unexpected argument"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: remove**
  - **Usage**: `symbrain profile remove <name> [--force]`
  - **Flags**: `--force`: Skip confirmation prompt
  - **Exit codes**:
    - Missing `<name>`: Exit 64 (ExitUSAGE) - "unexpected argument"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### setup
- **Usage**: `symbrain setup [flags]`
- **Flags**:
  - `-allow-unsigned`: Install even if cosign/signature unavailable
  - `-fix`: Repair missing/version-mismatched binaries
  - `-force-release`: With `-fix`: allow replacing brain-source build
  - `-from-source string`: Build optional modules from in-repo sources
  - `-json`: Emit machine-readable JSON
  - `-modules string`: Module selection for `-from-source` (browse,operate,scope)
- **Exit codes**:
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + setup usage
  - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + setup usage
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### harness
- **Usage**: `symbrain harness <subcommand> [flags]`
- **Subcommands**:
  - `list [--project DIR]`: List harnesses
  - `health [--harness NAME] [--project DIR]`: Health check harnesses
- **Global flags**: `--output table|json`, `--json`
- **Subcommand: list**
  - **Usage**: `symbrain harness list [--project DIR]`
  - **Flags**: `--project string`: Project directory for project-local config
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: health**
  - **Usage**: `symbrain harness health [--harness NAME] [--project DIR]`
  - **Flags**:
    - `--harness string`: Only probe servers of this harness
    - `--project string`: Project directory for project-local config
  - **Exit codes**: Success: `0` (ExitOK)

### usage
- **Usage**: `symbrain usage [--output table|json] [--json]`
- **Flags**:
  - `--output table|json`: Output format
  - `--json`: Shorthand for `--output json`
- **Exit codes**: Success: `0` (ExitOK)

### mcp
- **Usage**: `symbrain mcp [flags]`
- **Flags**:
  - `-profile string`: Profile name to serve (required unless `-profile-file` given)
  - `-profile-file string`: Load profile from TOML file
  - `-vault-agent string`: Vault agent name for stdio mode (default: harness-detected or "claude-code")
- **Exit codes**:
  - Missing `-profile`: Exit 64 (ExitUSAGE) - "flag provided but not defined: -profile" + mcp usage
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + mcp usage
  - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + mcp usage
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### install
- **Usage**: `symbrain install [flags]`
- **Flags**:
  - `-dry-run`: Print unified diff, write nothing
  - `-harness string`: Harness to install into (required: claude, claude-desktop, cursor, opencode, codex, antigravity)
  - `-keep-superseded`: Keep superseded entries instead of migrating
  - `-profile string`: Profile to bind (default: global config's default_profile)
  - `-project string`: Project directory (for project-local config like claude's .mcp.json)
- **Exit codes**:
  - Missing `-harness`: Exit 64 (ExitUSAGE) - "flag provided but not defined: -harness" + install usage
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + install usage
  - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + install usage
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### uninstall
- **Usage**: `symbrain uninstall [flags]`
- **Flags**: Same as `install` (shares implementation)
- **Exit codes**: Same as `install`

### sync
- **Usage**: `symbrain sync [flags]`
- **Flags**:
  - `-dry-run`: Show plan without writing
  - `-project string`: Project directory (default: current directory)
- **Exit codes**:
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + sync usage
  - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + sync usage
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### memory
- **Usage**: `symbrain memory <subcommand> [flags]`
- **Subcommands**:
  - `list`: List stored memories
  - `search <query>`: Search memories by semantic relevance
  - `set <content> --kind <kind>`: Store a memory
  - `delete <id>`: Remove a memory by ID
  - `rules`: List procedural rules
  - `query-log`: Inspect memory retrieval log
  - `sync`: Synchronize with remote memory server
  - `serve`: Run memory HTTP API as sync peer
- **Global flags**: `--output table|json`, `--json` (for subcommands that support it)
- **Subcommand: list**
  - **Usage**: `symbrain memory list [flags]`
  - **Flags**:
    - `--scope, -s <scope>`: Filter by scope (global, project, agent, user, session)
    - `--limit, -l <N>`: Max memories (default 100, max 1000)
    - `--db <path>`: Database path override
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: search**
  - **Usage**: `symbrain memory search <query> [flags]`
  - **Flags**: Same as `list` plus required `<query>` positional
  - **Exit codes**:
    - Missing `<query>`: Exit 64 (ExitUSAGE) - "unexpected argument"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: set**
  - **Usage**: `symbrain memory set <content> --kind <kind> [flags]`
  - **Flags**:
    - `--kind, -k <kind>`: Semantic kind (required: user, feedback, project, reference)
    - `--scope, -s <scope>`: Scope (global/default, project, agent, user, session)
    - `--author <name>`: Author (default: cli:symbrain)
    - `--metadata <json>`: JSON metadata object
    - `--entities <names>`: Comma-separated entity names to link
    - `--staged`: Store as candidate (excluded from retrieval until promoted)
    - `--db <path>`: Database path override
  - **Exit codes**:
    - Missing `<content>` or `--kind`: Exit 64 (ExitUSAGE) - appropriate error
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: delete**
  - **Usage**: `symbrain memory delete <id> [flags]`
  - **Flags**: `--db <path>`: Database path override
  - **Exit codes**:
    - Missing `<id>`: Exit 64 (ExitUSAGE) - "unexpected argument"
    - Extra arguments: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: rules**
  - **Usage**: `symbrain memory rules [flags]`
  - **Flags**: Same as `list` (scope, limit, db)
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: query-log**
  - **Usage**: `symbrain memory query-log [flags]`
  - **Flags**:
    - `--limit, -l <N>`: Max recent entries (default 50, max 1000)
    - `--actor <name>`: Filter by actor
    - `--db <path>`: Database path override
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: sync**
  - **Usage**: `symbrain memory sync --remote <url> [flags]`
  - **Flags**:
    - `--remote <url>`: Remote server URL (required, https except localhost)
    - `--pull`: Only pull remote changes
    - `--push`: Only push local changes
    - `--token <token>`: Bearer token (or $SYMBRAIN_MEMORY_SYNC_TOKEN)
    - `--encrypted-relay`: Exchange encrypted blobs via relay
    - `--relay-passphrase <p>`: Passphrase for encrypted relay
    - `--allow-insecure-http`: Override https requirement (with warning)
    - `--db <path>`: Database path override
    - `--timeout <duration>`: Per-run HTTP timeout (default 60s)
  - **Exit codes**:
    - Missing `--remote`: Exit 64 (ExitUSAGE) - "flag provided but not defined: -remote"
    - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + sync usage
    - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + sync usage
    - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"
- **Subcommand: serve**
  - **Usage**: `symbrain memory serve [flags]`
  - **Flags**:
    - `--port <n>`: TCP port (default 8787, bound to 127.0.0.1)
    - `--db <path>`: Database path override
  - **Exit codes**:
    - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + serve usage
    - Missing flag value: Exit 64 (ExitUSAGE) - "flag needs an argument: -{flag}" + serve usage
    - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg>"

### skills
- **Usage**: `symbrain skills <subcommand> [flags]`
- **Subcommands**:
  - `list`: List skills in library
  - `status`: Classify installed skills (drift report)
  - `targets`: Show harness targets for skills
  - `log`: Read skill operation log
  - `sync`: Repair drifted installs
  - `doctor`: Report configured skill paths and target roots
- **Global flags**: `--output table|json`, `--json` (for subcommands that support it)
- **Subcommand: list**
  - **Usage**: `symbrain skills list [flags]`
  - **Flags**:
    - `--scope string`: Install scope (user or project, default "user")
    - `--target string`: Limit to one harness target
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: status**
  - **Usage**: `symbrain skills status [flags]`
  - **Flags**: Same as `list` (scope, target)
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: targets**
  - **Usage**: `symbrain skills targets [flags]`
  - **Flags**: Same as `list` (scope, target)
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: log**
  - **Usage**: `symbrain skills log [flags]`
  - **Flags**:
    - `-l int` / `-limit int`: Max records (newest first)
    - `--scope string`: Install scope (user/project)
    - `--skill string`: Limit to one skill name
    - `--target string`: Limit to one harness target
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: sync**
  - **Usage**: `symbrain skills sync [flags]`
  - **Flags**:
    - `--dry-run`: Report plan without writing
    - `--scope string`: Install scope (user/project)
    - `--target string`: Limit to one harness target
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: doctor**
  - **Usage**: `symbrain skills doctor [flags]`
  - **Flags**: Same as `list` (scope, target)
  - **Exit codes**: Success: `0` (ExitOK)
- **Note**: All skills subcommands accept global `--output table|json` and `--json`

### activity
- **Usage**: `symbrain activity <subcommand> [flags]`
- **Subcommands**:
  - `search`: Search activity
  - `get`: Get activity
  - `status`: Activity status
- **Note**: Every command requires `--profile`, explicit bounded response budget, and (for search) RFC3339 window and result limit
- **Exit codes**: Success: `0` (ExitOK) for valid usage

### audit
- **Usage**: `symbrain audit [flags]`
- **Flags**: No command-specific flags (only global `--output table|json` and `--json`)
- **Exit codes**:
  - Success: `0` (ExitOK)
  - Unknown flag: Exit 64 (ExitUSAGE) - "flag provided but not defined: -{flag}" + audit usage
  - Missing flag value: Not applicable (no value flags)
  - Extra positional: Exit 64 (ExitUSAGE) - "unexpected argument {arg}"

### vault
- **Usage**: `symbrain vault <subcommand> [flags]`
- **Subcommands**:
  - `create <path>`: Create secret from stdin
  - `set <path.field>`: Set secret from stdin
  - `delete <path.field>`: Delete secret (requires `--yes`)
- **Note**: The vault commands are pass-through to the symvault binary; help output comes from symvault, not symbrain
- **Exit codes**: Vary based on symvault behavior

### guard
- **Usage**: `symbrain guard <subcommand> [flags]`
- **Subcommands**:
  - `version`: Print version and build info
  - `doctor`: Check system health and configuration
  - `decide`: Read JSON decision from stdin, write JSON decision to stdout
  - `grants`: List and revoke standing grants
  - `scan`: Discover MCP servers across AI clients
  - `help`: Show help message
- **Subcommand: version**
  - **Usage**: `symbrain guard version`
  - **Flags**: None
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: doctor**
  - **Usage**: `symbrain guard doctor`
  - **Flags**: None
  - **Exit codes**: Success: `0` (ExitOK) when healthy, non-zero when issues found
- **Subcommand: decide**
  - **Usage**: `symbrain guard decide < request.json`
  - **Flags**: None
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: grants**
  - **Usage**: `symbrain grants list` or `symbrain grants revoke <id> | --all`
  - **Flags**: `--all`: Revoke all grants
  - **Exit codes**: Success: `0` (ExitOK)
- **Subcommand: scan**
  - **Usage**: `symbrain guard scan [--format table|json]`
  - **Flags**: `--format table|json`: Output format
  - **Exit codes**: Success: `0` (ExitOK)

## Flag Behaviors

### Boolean Flags
- Format: `-flag` or `--flag`
- Present = true, absent = false
- Examples: `-json`, `-dry-run`, `-fix`, `-force-release`, `--preview`, `--no-backup`, `--staged`, `--pull`, `--push`, `--encrypted-relay`, `--allow-insecure-http`, `--force`

### Value Flags
- Format: `-flag value`, `--flag value`, `-flag=value`, `--flag=value`
- Examples: `--project DIR`, `--harness NAME`, `-profile string`, `-vault-agent string`, `--scope string`, `--target string`, `--author <name>`, `--metadata <json>`, `--entities <names>`, `--token <token>`, `--relay-passphrase <p>`, `--timeout <duration>`, `--port <n>`, `--db <path>`, `--limit <N>`, `-l <N>`, `--skill string`, `--author <name>`, `--from string`

### Shorthands
- `-json` = `--output json`
- `-s` = `--scope` (in memory, skills, audit contexts)
- `-l` = `--limit` (in memory, skills contexts)
- `-k` = `--kind` (in memory set context)

### Aliases
- `serve` (deprecated) = `mcp` (prints deprecation warning to stderr)

### Terminator Behavior
- `--`: Treats all following arguments as positional (even if they look like flags)
- `-`: Treated as positional argument (stdin indicator in some contexts)

### Unknown Flag Handling
- Exit code: `64` (ExitUSAGE)
- stderr: `"flag provided but not defined: -{flag}\\n"` + command-specific usage
- stdout: empty (unless command normally outputs to stdout)

### Missing Flag Value Handling
- Exit code: `64` (ExitUSAGE)
- stderr: `"flag needs an argument: -{flag}\\n"` + command-specific usage
- stdout: empty

### Unknown Subcommand Handling
- Exit code: `64` (ExitUSAGE)
- stderr: `"symbrain: unknown command {cmd}\\n\\n"` + full usage
- stdout: empty

### Missing Subcommand (No Arguments)
- Exit code: `100` (ExitNoInput)
- stderr: empty
- stdout: full usage

### Extra Positional Arguments
- Exit code: `64` (ExitUSAGE)
- stderr: `"symbrain {command}: unexpected argument {arg:\\"\\"}\\n"` + command usage (if applicable)
- stdout: empty

### Help Flags (`-h`, `--help`, `help`)
- For commands with subcommands: prints subcommand usage
- For top-level: prints full usage
- Exit code: `64` (ExitUSAGE) except for top-level `help` which returns `0` (ExitOK)
- stdout: usage text
- stderr: empty (unless error condition)

## Output Format Flags (`--output table|json`, `--json`)
- Supported by: version, sync, memory, skills, activity, profile, harness, audit, usage, doctor
- `--json`: Shorthand for `--output json`
- Default format: `table`
- JSON output: Compact JSON with `&`, `<`, `>` escaped (no trailing newline)