package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/conflict"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// defaultMemoryAuthor attributes writes that arrive without an explicit
// --author. The CLI is a human-driven surface, so the default names the
// command rather than an agent identity.
const defaultMemoryAuthor = "cli:symbrain"

// openMemoryService opens the embedded memory store and wraps it in the same
// service the MCP gateway uses, so a CLI write goes through exactly the write
// path (embedding, PII redaction, conflict detection, governance) that a
// harness write does. The caller closes the returned database.
func openMemoryService(path string) (*memorymcp.MemoryService, *db.DB, error) {
	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	if path != "" {
		memcfg.Database.Path = path
	}
	database, err := db.Open(memcfg)
	if err != nil {
		return nil, nil, err
	}
	service := memorymcp.NewMemoryService(database, extractor.NewEmbeddingsGenerator(memcfg), true)
	service.SetRankingWeights(db.WeightsFromConfig(memcfg.Ranking))
	if memcfg.Conflict.Enabled {
		service.SetConflictChecker(conflict.NewChecker(database, memcfg.Conflict))
	}
	return service, database, nil
}

// memorySetResult is the JSON shape of `symbrain memory set`.
type memorySetResult struct {
	ID     string `json:"id"`
	Scope  string `json:"scope"`
	Kind   string `json:"kind"`
	Staged bool   `json:"staged"`
}

// memoryDeleteResult is the JSON shape of `symbrain memory delete`.
type memoryDeleteResult struct {
	ID      string `json:"id"`
	Deleted bool   `json:"deleted"`
}

func cmdMemorySetWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemorySetUsage(stdout)
		return exitcodes.ExitOK
	}
	args = reorderFlagsFirst(args, map[string]bool{
		"scope": true, "s": true, "kind": true, "k": true,
		"author": true, "metadata": true, "entities": true, "db": true,
	})

	fs := flag.NewFlagSet("memory set", flag.ContinueOnError)
	fs.SetOutput(stderr)
	scope := fs.String("scope", "global", "scope: global, project, agent, user, or session")
	fs.StringVar(scope, "s", "global", "scope: global, project, agent, user, or session")
	kind := fs.String("kind", "", "semantic kind: user, feedback, project, or reference (required)")
	fs.StringVar(kind, "k", "", "semantic kind: user, feedback, project, or reference (required)")
	author := fs.String("author", defaultMemoryAuthor, "author recorded on the memory")
	metadata := fs.String("metadata", "", "optional JSON object of metadata key/value pairs")
	entities := fs.String("entities", "", "optional comma-separated entity names to link")
	staged := fs.Bool("staged", false, "store as a staged candidate, excluded from retrieval until promoted")
	dbPath := fs.String("db", "", "database path override (default: the configured memory database)")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain memory set <content> --kind <kind> [--scope <scope>] [flags]")
		return exitcodes.ExitNoInput
	}
	content := strings.TrimSpace(fs.Arg(0))
	if content == "" {
		fmt.Fprintln(stderr, "symbrain memory set: content is required")
		return exitcodes.ExitNoInput
	}
	if *kind == "" {
		fmt.Fprintf(stderr, "symbrain memory set: --kind is required (one of: %s)\n", strings.Join(db.ValidKinds(), ", "))
		return exitcodes.ExitNoInput
	}
	canonicalKind, ok := db.NormalizeKind(*kind)
	if !ok {
		fmt.Fprintf(stderr, "symbrain memory set: invalid kind %q (valid: %s)\n", *kind, strings.Join(db.ValidKinds(), ", "))
		return exitcodes.ExitNoInput
	}

	meta := map[string]string{}
	if strings.TrimSpace(*metadata) != "" {
		if err := json.Unmarshal([]byte(*metadata), &meta); err != nil {
			fmt.Fprintf(stderr, "symbrain memory set: --metadata must be a JSON object: %v\n", err)
			return exitcodes.ExitNoInput
		}
	}

	var entityNames []string
	for _, name := range strings.Split(*entities, ",") {
		if name = strings.TrimSpace(name); name != "" {
			entityNames = append(entityNames, name)
		}
	}

	service, database, err := openMemoryService(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory set: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	id, err := service.SetGoverned(content, *scope, meta, "", *author, entityNames, "symbrain-cli", false, 0, canonicalKind, *staged)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory set: store memory: %v\n", err)
		return exitcodes.ExitGeneric
	}

	result := memorySetResult{ID: id, Scope: *scope, Kind: canonicalKind, Staged: *staged}
	rows := output.Rows{
		JSON: result,
		Table: func(w io.Writer) error {
			state := "stored"
			if *staged {
				state = "staged for review"
			}
			_, err := fmt.Fprintf(w, "Memory %s (%s, %s, %s).\n", id, *scope, canonicalKind, state)
			return err
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory set: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func cmdMemoryDeleteWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemoryDeleteUsage(stdout)
		return exitcodes.ExitOK
	}
	args = reorderFlagsFirst(args, map[string]bool{"db": true})

	fs := flag.NewFlagSet("memory delete", flag.ContinueOnError)
	fs.SetOutput(stderr)
	dbPath := fs.String("db", "", "database path override (default: the configured memory database)")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain memory delete <id> [--db <path>]")
		return exitcodes.ExitNoInput
	}
	id := strings.TrimSpace(fs.Arg(0))
	if id == "" {
		fmt.Fprintln(stderr, "symbrain memory delete: id is required")
		return exitcodes.ExitNoInput
	}

	service, database, err := openMemoryService(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory delete: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	if err := service.Delete(id); err != nil {
		fmt.Fprintf(stderr, "symbrain memory delete: %v\n", err)
		return exitcodes.ExitGeneric
	}

	rows := output.Rows{
		JSON: memoryDeleteResult{ID: id, Deleted: true},
		Table: func(w io.Writer) error {
			_, err := fmt.Fprintf(w, "Deleted memory %s.\n", id)
			return err
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory delete: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printMemorySetUsage(w io.Writer) {
	fmt.Fprintf(w, `symbrain memory set — store a memory in the embedded store

Usage:
  symbrain memory set <content> --kind <kind> [flags]

Flags:
  --kind, -k <kind>    Semantic kind: %s (required).
  --scope, -s <scope>  Scope: global (default), project, agent, user, or session.
  --author <name>      Author recorded on the memory (default %s).
  --metadata <json>    JSON object of metadata key/value pairs.
  --entities <names>   Comma-separated entity names to link.
  --staged             Store as a candidate, excluded from retrieval until promoted.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
`, strings.Join(db.ValidKinds(), ", "), defaultMemoryAuthor)
}

func printMemoryDeleteUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory delete — remove a memory from the embedded store

Usage:
  symbrain memory delete <id> [flags]

Flags:
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
`)
}
