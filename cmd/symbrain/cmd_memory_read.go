package main

import (
	"flag"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// openMemoryDatabase opens the embedded memory store used by read-only memory
// commands. A --db override is provided so callers can inspect an explicit
// store without changing the configured default.
func openMemoryDatabase(path string) (*db.DB, error) {
	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	if path != "" {
		memcfg.Database.Path = path
	}
	return db.Open(memcfg)
}

func memoryReadFlags(name string, args []string, stderr io.Writer) (*flag.FlagSet, *string, *int, *string, bool) {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	scope := fs.String("scope", "", "filter by scope: global, project, agent, user, or session")
	fs.StringVar(scope, "s", "", "filter by scope: global, project, agent, user, or session")
	limit := fs.Int("limit", 0, "maximum number of memories to return")
	fs.IntVar(limit, "l", 0, "maximum number of memories to return")
	dbPath := fs.String("db", "", "database path override (default: the configured memory database)")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return fs, scope, limit, dbPath, false
	}
	return fs, scope, limit, dbPath, true
}

// cmdMemoryList implements `symbrain memory list` using the same lightweight
// keyset-backed database path as the MCP memory_list handler.
func cmdMemoryList(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory list: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdMemoryListWithFormat(args, stdout, stderr, format)
}

func cmdMemoryListWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemoryListUsage(stdout)
		return exitcodes.ExitOK
	}
	fs, scope, limit, dbPath, ok := memoryReadFlags("memory list", args, stderr)
	if !ok {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain memory list: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}
	if *limit <= 0 {
		*limit = 100
	}
	if *limit > 1000 {
		*limit = 1000
	}

	database, err := openMemoryDatabase(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory list: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	memories, err := database.ListMemoriesLiteWithCursor(*scope, nil, *limit)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory list: list memories: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if memories == nil {
		memories = make([]*db.Memory, 0)
	}
	// Match the MCP response boundary: records from legacy/import paths are
	// redacted immediately before they leave the process.
	security.RedactMemories(memories)

	rows := output.Rows{
		JSON: memories,
		Table: func(w io.Writer) error {
			printMemoryListTable(w, memories)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory list: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printMemoryListTable(w io.Writer, memories []*db.Memory) {
	if len(memories) == 0 {
		fmt.Fprintln(w, "No memories found.")
		return
	}
	fmt.Fprintln(w, "ID\tSCOPE\tCREATED\tCONTENT")
	for _, m := range memories {
		if m == nil {
			continue
		}
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", m.ID, m.Scope, m.CreatedAt.UTC().Format(time.RFC3339), tableMemoryContent(m.Content))
	}
}

// cmdMemorySearch implements `symbrain memory search <query>` using the
// existing vector-ranked DB search entry point. It deliberately does not add
// or alter ranking, filtering, or embedding behavior.
func cmdMemorySearch(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory search: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdMemorySearchWithFormat(args, stdout, stderr, format)
}

func cmdMemorySearchWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemorySearchUsage(stdout)
		return exitcodes.ExitOK
	}
	// Accept the query in the documented position while allowing flags before
	// or after it, matching the rest of the CLI's positional-command behavior.
	args = reorderFlagsFirst(args, map[string]bool{"scope": true, "s": true, "limit": true, "l": true, "db": true})
	fs, scope, limit, dbPath, ok := memoryReadFlags("memory search", args, stderr)
	if !ok {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain memory search <query> [--scope <scope>] [--limit <N>] [--db <path>]")
		return exitcodes.ExitNoInput
	}
	query := fs.Arg(0)
	if query == "" {
		fmt.Fprintln(stderr, "symbrain memory search: query is required")
		return exitcodes.ExitNoInput
	}
	if *limit <= 0 {
		*limit = 5
	}

	memcfg, err := config.Load()
	if err != nil {
		memcfg = config.Defaults()
	}
	database, err := openMemoryDatabase(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory search: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	embeddings := extractor.NewEmbeddingsGenerator(memcfg)
	emb := embeddings.GenerateVector(query)
	results, err := database.SearchMemories(emb.Vector, emb.Source, *scope, *limit)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory search: search memories: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if results == nil {
		results = make([]db.SearchResult, 0)
	}
	// Match the MCP response boundary for every ranked result, including data
	// written before write-time PII redaction was enabled.
	security.RedactSearchResults(results)
	for i := range results {
		if results[i].Memory != nil {
			// Search hydrates embeddings for ranking, but they are not part of
			// the consumer-facing CLI result (and MCP never returns vectors).
			results[i].Memory.Embedding = nil
			results[i].Memory.EmbeddingBinary = nil
		}
	}

	rows := output.Rows{
		JSON: results,
		Table: func(w io.Writer) error {
			printMemorySearchTable(w, results)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory search: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printMemorySearchTable(w io.Writer, results []db.SearchResult) {
	if len(results) == 0 {
		fmt.Fprintln(w, "No relevant memories found.")
		return
	}
	fmt.Fprintln(w, "SCORE\tID\tSCOPE\tCREATED\tCONTENT")
	for _, result := range results {
		if result.Memory == nil {
			continue
		}
		fmt.Fprintf(w, "%.6f\t%s\t%s\t%s\t%s\n", result.Score, result.Memory.ID, result.Memory.Scope, result.Memory.CreatedAt.UTC().Format(time.RFC3339), tableMemoryContent(result.Memory.Content))
	}
}

func tableMemoryContent(content string) string {
	return strings.NewReplacer("	", " ", "\r", " ", "\n", " ").Replace(content)
}

func hasMemoryHelpFlag(args []string) bool {
	for _, arg := range args {
		if arg == "-h" || arg == "--help" {
			return true
		}
	}
	return false
}

func printMemoryListUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory list — list stored memories

Usage:
  symbrain memory list [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum memories to return (default 100, max 1000).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
`)
}

func printMemorySearchUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory search — search memories by semantic relevance

Usage:
  symbrain memory search <query> [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum results to return (default 5).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
`)
}
