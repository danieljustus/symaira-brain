package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	memorydb "github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/patterns"
)

type validationCase struct {
	ID        string `json:"id"`
	Query     string `json:"query"`
	From      string `json:"from"`
	To        string `json:"to"`
	Limit     int    `json:"limit"`
	MaxTokens int    `json:"max_tokens"`
	Error     string `json:"error,omitempty"`
}

type patternCase struct {
	Episodes  []patterns.Episode `json:"episodes"`
	Threshold int                `json:"threshold"`
	Patterns  []patterns.Pattern `json:"patterns"`
	Names     []string           `json:"names"`
	StoreLine string             `json:"store_line"`
}

type activityCase struct {
	Validation []validationCase    `json:"validation"`
	Items      []activity.ReadItem `json:"items"`
	Page       activity.SearchPage `json:"page"`
}

type suite struct {
	Patterns patternCase  `json:"patterns"`
	Activity activityCase `json:"activity"`
}

func step(server, tool string) patterns.Step {
	return patterns.Step{Server: server, Tool: tool}
}

func patternFixture() patternCase {
	sequenceA := []patterns.Step{step("memory", "memory_search"), step("vault", "request_credential")}
	sequenceB := []patterns.Step{step("skills", "skills_list"), step("skills", "skills_install"), step("skills", "skills_status"), step("skills", "skills_log")}
	episodes := []patterns.Episode{
		{Profile: "personal", Steps: sequenceA, StartedAt: "2026-08-01T09:00:00Z", EndedAt: "2026-08-01T09:05:00Z"},
		{Profile: "personal", Steps: sequenceA, StartedAt: "2026-08-02T09:00:00Z", EndedAt: "2026-08-02T09:05:00Z"},
		{Profile: "personal", Steps: sequenceA, StartedAt: "2026-08-03T09:00:00Z", EndedAt: "2026-08-03T09:05:00Z"},
		{Profile: "personal", Steps: sequenceB, StartedAt: "2026-08-04T09:00:00Z", EndedAt: "2026-08-04T09:05:00Z"},
		{Profile: "personal", Steps: sequenceB, StartedAt: "2026-08-05T09:00:00Z", EndedAt: "2026-08-05T09:05:00Z"},
		{Profile: "personal", Steps: sequenceB, StartedAt: "2026-08-06T09:00:00Z", EndedAt: "2026-08-06T09:05:00Z"},
		{Profile: "personal", Steps: []patterns.Step{step("vault", "health")}, StartedAt: "2026-08-07T09:00:00Z", EndedAt: "2026-08-07T09:05:00Z"},
	}
	promoted := patterns.Promote(episodes, 3)
	names := []string{patterns.Name("personal", sequenceA), patterns.Name("personal", sequenceB)}
	dir, err := os.MkdirTemp("", "patterns-oracle")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	path := filepath.Join(dir, "personal.jsonl")
	if err := patterns.NewStore(path).Append(episodes[0]); err != nil {
		panic(err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	return patternCase{Episodes: episodes, Threshold: 3, Patterns: promoted, Names: names, StoreLine: string(data)}
}

func validationFixture() []validationCase {
	base := time.Date(2026, 8, 1, 9, 0, 0, 0, time.UTC)
	cases := []validationCase{
		{ID: "valid", Query: "editor", From: base.Format(time.RFC3339), To: base.Add(time.Hour).Format(time.RFC3339), Limit: 2, MaxTokens: 20},
		{ID: "query-required", Query: " ", From: base.Format(time.RFC3339), To: base.Add(time.Hour).Format(time.RFC3339), Limit: 2, MaxTokens: 20},
		{ID: "query-too-long", Query: string(make([]rune, activity.MaxQueryLength+1)), From: base.Format(time.RFC3339), To: base.Add(time.Hour).Format(time.RFC3339), Limit: 2, MaxTokens: 20},
		{ID: "window-order", Query: "x", From: base.Format(time.RFC3339), To: base.Format(time.RFC3339), Limit: 2, MaxTokens: 20},
		{ID: "window-range", Query: "x", From: base.Format(time.RFC3339), To: base.Add(activity.MaxRange + time.Nanosecond).Format(time.RFC3339Nano), Limit: 2, MaxTokens: 20},
		{ID: "limit", Query: "x", From: base.Format(time.RFC3339), To: base.Add(time.Hour).Format(time.RFC3339), Limit: 0, MaxTokens: 20},
		{ID: "tokens", Query: "x", From: base.Format(time.RFC3339), To: base.Add(time.Hour).Format(time.RFC3339), Limit: 2, MaxTokens: 0},
	}
	for i := range cases {
		from, _ := time.Parse(time.RFC3339Nano, cases[i].From)
		to, _ := time.Parse(time.RFC3339Nano, cases[i].To)
		err := activity.ValidateSearchOptions(activity.SearchOptions{Query: cases[i].Query, From: from, To: to, Limit: cases[i].Limit, MaxTokens: cases[i].MaxTokens})
		if err != nil {
			cases[i].Error = err.Error()
		}
	}
	return cases
}

func activityFixture() activityCase {
	base := time.Date(2026, 8, 1, 9, 0, 0, 0, time.UTC)
	cfg := memoryconfig.Defaults()
	dir, err := os.MkdirTemp("", "activity-oracle")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	cfg.Database.Path = filepath.Join(dir, "activity.db")
	database, err := memorydb.Open(cfg)
	if err != nil {
		panic(err)
	}
	defer database.Close()
	store, err := activity.NewStore(database, activity.Options{Now: func() time.Time { return base }})
	if err != nil {
		panic(err)
	}
	segments := []activity.Segment{
		{ID: "z", Source: "symcockpit", Granularity: activity.Granularity10Min, StartedAt: base.Add(time.Hour), EndedAt: base.Add(70 * time.Minute), Applications: []string{"Editor"}, RedactedSummary: "editor second", ExpiresAt: base.Add(24 * time.Hour)},
		{ID: "a", Source: "symcockpit", Granularity: activity.Granularity10Min, StartedAt: base, EndedAt: base.Add(10 * time.Minute), Applications: []string{"Editor"}, RedactedSummary: "editor ok", ExpiresAt: base.Add(24 * time.Hour)},
	}
	for _, segment := range segments {
		if err := store.SaveSegment(segment); err != nil {
			panic(err)
		}
	}
	items := make([]activity.ReadItem, 0, len(segments))
	for _, segment := range segments {
		item, err := store.GetReadItem(segment.ID)
		if err != nil || item == nil {
			panic("get activity fixture")
		}
		items = append(items, *item)
	}
	page, err := store.Search(activity.SearchOptions{Query: "editor", From: base.Add(-time.Minute), To: base.Add(2 * time.Hour), Limit: 2, MaxTokens: 5})
	if err != nil {
		panic(err)
	}
	return activityCase{Validation: validationFixture(), Items: items, Page: page}
}

func generate() suite {
	return suite{Patterns: patternFixture(), Activity: activityFixture()}
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-activity/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()
	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		panic(err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/patterns-activity-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: patterns/activity oracle deterministic check passed")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		panic(err)
	}
	fmt.Printf("Wrote %s\n", *output)
}
