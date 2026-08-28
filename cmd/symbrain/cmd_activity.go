package main

import (
	"flag"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	memorydb "github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/policy"
	profilepkg "github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

const cliActivityMaxBudget = activity.MaxTokens
const cliActivityBudgetFlag = "max-" + "tokens"
const cliActivityFenceStart = "[UNTRUSTED_ACTIVITY_SUMMARY]"
const cliActivityFenceEnd = "[/UNTRUSTED_ACTIVITY_SUMMARY]"

func cmdActivity(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if len(args) == 0 || args[0] == "--help" || args[0] == "-h" {
		printActivityUsage(stdout)
		if len(args) == 0 {
			return exitcodes.ExitNoInput
		}
		return exitcodes.ExitOK
	}
	if !activityProfileAllowed(args) {
		fmt.Fprintln(stderr, "symbrain activity: --profile is required and must explicitly expose activity read tools")
		return exitcodes.ExitNoInput
	}
	switch args[0] {
	case "search":
		return cmdActivitySearch(args[1:], stdout, stderr, format)
	case "get":
		return cmdActivityGet(args[1:], stdout, stderr, format)
	case "status":
		return cmdActivityStatus(args[1:], stdout, stderr, format)
	default:
		fmt.Fprintf(stderr, "symbrain activity: unknown subcommand %q\n", args[0])
		printActivityUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

func activityProfileAllowed(args []string) bool {
	name := ""
	for i := 0; i < len(args); i++ {
		if args[i] == "--profile" && i+1 < len(args) {
			name = args[i+1]
			i++
		} else if strings.HasPrefix(args[i], "--profile=") {
			name = strings.TrimPrefix(args[i], "--profile=")
		}
	}
	if name == "" {
		return false
	}
	p, err := profilepkg.Load(name)
	if err != nil {
		return false
	}
	report, err := policy.EvaluatePreset(profilepkg.ServerMemory, p.Server(profilepkg.ServerMemory))
	if err != nil {
		return false
	}
	for _, tool := range report.Exposed {
		if tool == "activity_search" || tool == "activity_get" || tool == "activity_status" {
			return true
		}
	}
	return false
}

func activityFlags(name string, args []string, stderr io.Writer) (*flag.FlagSet, *string, *string, *string, *string, *int, *int, bool) {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	profile := fs.String("profile", "", "profile that explicitly exposes activity read tools")
	dbPath := fs.String("db", "", "database path override")
	from := fs.String("from", "", "required RFC3339 window start")
	to := fs.String("to", "", "required RFC3339 window end (at most 7 days)")
	limit := fs.Int("limit", 0, "required result limit (1-50)")
	maxTokens := fs.Int("max-tokens", 0, "required response token budget (1-4000)")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return fs, profile, dbPath, from, to, limit, maxTokens, false
	}
	return fs, profile, dbPath, from, to, limit, maxTokens, true
}

func openActivityStore(path string) (*activity.Store, *memorydb.DB, error) {
	cfg, err := memoryconfig.Load()
	if err != nil {
		cfg = memoryconfig.Defaults()
	}
	if path != "" {
		cfg.Database.Path = path
	}
	database, err := memorydb.Open(cfg)
	if err != nil {
		return nil, nil, err
	}
	store, err := activity.NewStoreFromConfig(database, cfg)
	if err != nil {
		_ = database.Close()
		return nil, nil, err
	}
	return store, database, nil
}

func cmdActivitySearch(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, dbPath, fromText, toText, limit, maxTokens, ok := activityFlags("activity search", args, stderr)
	if !ok || fs.NArg() != 1 {
		fmt.Fprintf(stderr, "usage: symbrain activity search <query> --profile <name> --from <RFC3339> --to <RFC3339> --limit <N> --%s <N> [--db <path>]\n", cliActivityBudgetFlag)
		return exitcodes.ExitNoInput
	}
	if *limit < 1 || *limit > activity.MaxResults || *maxTokens < 1 || *maxTokens > cliActivityMaxBudget {
		fmt.Fprintf(stderr, "symbrain activity search: invalid bounds (limit 1-%d; budget 1-%d)\n", activity.MaxResults, cliActivityMaxBudget)
		return exitcodes.ExitNoInput
	}
	from, to, err := parseCLIActivityWindow(*fromText, *toText)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity search: %v\n", err)
		return exitcodes.ExitNoInput
	}
	opts := activity.SearchOptions{Query: fs.Arg(0), From: from, To: to, Limit: *limit, MaxTokens: *maxTokens}
	if err := activity.ValidateSearchOptions(opts); err != nil {
		fmt.Fprintf(stderr, "symbrain activity search: %v\n", err)
		return exitcodes.ExitNoInput
	}
	store, database, err := openActivityStore(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity search: open database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()
	page, err := store.Search(opts)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity search: search: %v\n", err)
		return exitcodes.ExitGeneric
	}
	for i := range page.Results {
		page.Results[i].Summary = fenceCLIActivitySummary(page.Results[i].Summary, *maxTokens)
		page.Results[i].Tokens = cliActivityTokenCount(page.Results[i].Summary)
	}
	page.UsedTokens = 0
	for _, item := range page.Results {
		page.UsedTokens += item.Tokens
	}
	page.MaxTokens = *maxTokens
	return renderCLIActivity(stdout, format, page, func(w io.Writer) {
		for _, item := range page.Results {
			fmt.Fprintf(w, "%s\t%s\t%s\n", item.StartedAt.Format(time.RFC3339), item.Kind, security.Redact(item.Summary))
		}
	})
}

func cmdActivityGet(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, dbPath, _, _, _, maxTokens, ok := activityFlags("activity get", args, stderr)
	if !ok || fs.NArg() != 1 || *maxTokens < 1 || *maxTokens > cliActivityMaxBudget {
		fmt.Fprintf(stderr, "usage: symbrain activity get <id> --profile <name> --%s <N> [--db <path>]\n", cliActivityBudgetFlag)
		return exitcodes.ExitNoInput
	}
	store, database, err := openActivityStore(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity get: open database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()
	item, err := store.GetReadItem(fs.Arg(0))
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity get: get: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if item == nil {
		fmt.Fprintf(stderr, "symbrain activity get: activity not found: %s\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}
	item.Summary = fenceCLIActivitySummary(item.Summary, *maxTokens)
	item.Tokens = cliActivityTokenCount(item.Summary)
	return renderCLIActivity(stdout, format, item, func(w io.Writer) {
		fmt.Fprintf(w, "%s\t%s\t%s\n", item.StartedAt.Format(time.RFC3339), item.Kind, item.Summary)
	})
}

func cmdActivityStatus(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, dbPath, _, _, _, maxTokens, ok := activityFlags("activity status", args, stderr)
	if !ok || fs.NArg() != 0 || *maxTokens < 1 || *maxTokens > cliActivityMaxBudget {
		fmt.Fprintf(stderr, "usage: symbrain activity status --profile <name> --%s <N> [--db <path>]\n", cliActivityBudgetFlag)
		return exitcodes.ExitNoInput
	}
	store, database, err := openActivityStore(*dbPath)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity status: open database: %v\n", err)
		return exitcodes.ExitGeneric
	}
	defer func() { _ = database.Close() }()
	status, err := store.Status()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain activity status: status: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return renderCLIActivity(stdout, format, status, func(w io.Writer) {
		fmt.Fprintf(w, "segments=%d\tepisodes=%d\n", status.ActiveSegments, status.ActiveEpisodes)
	})
}

func parseCLIActivityWindow(fromText, toText string) (time.Time, time.Time, error) {
	from, err := time.Parse(time.RFC3339Nano, fromText)
	if err != nil {
		return time.Time{}, time.Time{}, fmt.Errorf("from must be RFC3339: %w", err)
	}
	to, err := time.Parse(time.RFC3339Nano, toText)
	if err != nil {
		return time.Time{}, time.Time{}, fmt.Errorf("to must be RFC3339: %w", err)
	}
	return from.UTC(), to.UTC(), nil
}

func fenceCLIActivitySummary(summary string, maxBudget int) string {
	start := cliActivityFenceStart + "\n"
	end := "\n" + cliActivityFenceEnd
	maxRunes := maxBudget * 4
	body := []rune(summary)
	budget := maxRunes - len([]rune(start)) - len([]rune(end))
	if budget < 0 {
		budget = 0
	}
	if len(body) > budget {
		body = body[:budget]
	}
	return start + string(body) + end
}

func cliActivityTokenCount(value string) int {
	if value == "" {
		return 0
	}
	return len([]rune(value))/4 + 1
}

func renderCLIActivity(stdout io.Writer, format output.Format, value any, table func(io.Writer)) exitcodes.ExitCode {
	if err := output.Render(stdout, format, output.Rows{JSON: value, Table: func(w io.Writer) error { table(w); return nil }}); err != nil {
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func printActivityUsage(w io.Writer) {
	fmt.Fprint(w, "symbrain activity — bounded, profile-gated activity reads\n\nUsage:\n  symbrain activity <search|get|status> [flags]\n\nEvery command requires --profile, an explicit bounded response budget, and (for search) an explicit RFC3339 window and result limit.\n")
}
