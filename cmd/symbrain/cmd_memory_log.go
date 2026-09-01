package main

import (
	"flag"
	"fmt"
	"io"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// cmdMemoryRulesWithFormat implements `symbrain memory rules` — the procedural
// rules stored alongside memories. Rules are read through the same redaction
// boundary as `memory list`: records written before write-time redaction
// existed are cleaned on the way out.
func cmdMemoryRulesWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemoryRulesUsage(stdout)
		return exitcodes.ExitOK
	}
	fs := flag.NewFlagSet("memory rules", flag.ContinueOnError)
	fs.SetOutput(stderr)
	scope := fs.String("scope", "", "filter by scope: global, project, agent, user, or session")
	fs.StringVar(scope, "s", "", "filter by scope: global, project, agent, user, or session")
	dbPath := fs.String("db", "", "database path override (default: the configured memory database)")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain memory rules: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	database, err := openMemoryDatabase(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory rules: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	rules, err := database.ListRules(*scope)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory rules: list rules: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if rules == nil {
		rules = make([]*db.Rule, 0)
	}
	for _, rule := range rules {
		if rule == nil {
			continue
		}
		rule.Content = security.Redact(rule.Content)
		rule.Metadata = security.RedactMap(rule.Metadata)
	}

	rows := output.Rows{
		JSON: rules,
		Table: func(w io.Writer) error {
			if len(rules) == 0 {
				_, err := fmt.Fprintln(w, "No rules found.")
				return err
			}
			if _, err := fmt.Fprintln(w, "ID\tSCOPE\tCREATED\tCONTENT"); err != nil {
				return err
			}
			for _, rule := range rules {
				if rule == nil {
					continue
				}
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", rule.ID, rule.Scope,
					rule.CreatedAt.UTC().Format(time.RFC3339), tableMemoryContent(rule.Content)); err != nil {
					return err
				}
			}
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory rules: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

// cmdMemoryQueryLogWithFormat implements `symbrain memory query-log` — the
// retrieval log recording which tool asked the memory store what, and on
// whose behalf.
func cmdMemoryQueryLogWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if hasMemoryHelpFlag(args) {
		printMemoryQueryLogUsage(stdout)
		return exitcodes.ExitOK
	}
	fs := flag.NewFlagSet("memory query-log", flag.ContinueOnError)
	fs.SetOutput(stderr)
	limit := fs.Int("limit", 0, "maximum recent entries to return")
	fs.IntVar(limit, "l", 0, "maximum recent entries to return")
	actor := fs.String("actor", "", "filter recent entries by actor")
	dbPath := fs.String("db", "", "database path override (default: the configured memory database)")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain memory query-log: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}
	if *limit <= 0 {
		*limit = 50
	}
	if *limit > 1000 {
		*limit = 1000
	}

	database, err := openMemoryDatabase(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory query-log: open memory database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()

	summary, err := database.GetQueryLogSummary(*limit, *actor)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain memory query-log: read query log: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if summary == nil {
		summary = &db.QueryLogSummary{RecentEntries: make([]*db.QueryLogEntry, 0)}
	}
	if summary.RecentEntries == nil {
		summary.RecentEntries = make([]*db.QueryLogEntry, 0)
	}

	rows := output.Rows{
		JSON: summary,
		Table: func(w io.Writer) error {
			if _, err := fmt.Fprintf(w, "Total queries: %d\n\n", summary.TotalQueries); err != nil {
				return err
			}
			if len(summary.RecentEntries) == 0 {
				_, err := fmt.Fprintln(w, "No recorded queries.")
				return err
			}
			if _, err := fmt.Fprintln(w, "WHEN\tTOOL\tACTOR\tMS\tQUERY"); err != nil {
				return err
			}
			for _, entry := range summary.RecentEntries {
				if entry == nil {
					continue
				}
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%d\t%s\n",
					entry.CreatedAt.UTC().Format(time.RFC3339), entry.Tool, entry.Actor,
					entry.DurationMs, tableMemoryContent(entry.QueryText)); err != nil {
					return err
				}
			}
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain memory query-log: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printMemoryRulesUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory rules — list procedural rules

Usage:
  symbrain memory rules [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
`)
}

func printMemoryQueryLogUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain memory query-log — inspect the memory retrieval log

Usage:
  symbrain memory query-log [flags]

Flags:
  --limit, -l <N>      Maximum recent entries to return (default 50, max 1000).
  --actor <name>       Filter recent entries by actor.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
`)
}
