package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func newCLIActivityTestDB(t *testing.T, base time.Time) string {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/activity.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open activity database: %v", err)
	}
	store, err := activity.NewStore(database, activity.Options{Now: func() time.Time { return base }})
	if err != nil {
		_ = database.Close()
		t.Fatalf("new activity store: %v", err)
	}
	if err := store.SaveSegment(activity.Segment{
		ID: "cli-segment", Source: "symcockpit", Granularity: activity.Granularity10Min,
		StartedAt: base, EndedAt: base.Add(10 * time.Minute),
		Applications: []string{"Editor"}, RedactedSummary: "edited activity summary",
		RawRef: "/opaque/cli.ref",
	}); err != nil {
		_ = database.Close()
		t.Fatalf("save activity segment: %v", err)
	}
	if err := store.SaveEpisode(activity.Episode{
		ID: "cli-episode", Title: "Editor episode", Scope: "project-a",
		StartedAt: base.Add(time.Hour), EndedAt: base.Add(2 * time.Hour),
		Confidence: 0.8, SegmentIDs: []string{"cli-segment"},
	}); err != nil {
		_ = database.Close()
		t.Fatalf("save activity episode: %v", err)
	}
	if err := database.Close(); err != nil {
		t.Fatalf("close activity database: %v", err)
	}
	return cfg.Database.Path
}

func writeActivityReadProfile(t *testing.T, home, name string, allow bool) {
	t.Helper()
	tools := ""
	if allow {
		tools = `tools_allow = ["activity_search", "activity_get", "activity_status"]`
	}
	writeProfileFile(t, home, name, `[profile]
name = "`+name+`"

[servers.memory]
enabled = true
mode = "read_only"
`+tools+`
`)
}

func TestCmdActivity_ReadCommandsAreBoundedAndProfileGated(t *testing.T) {
	home := sandboxHome(t)
	writeActivityReadProfile(t, home, "activity", true)
	writeActivityReadProfile(t, home, "restricted", false)
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	dbPath := newCLIActivityTestDB(t, base)

	t.Run("search json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"activity", "search", "--profile", "activity", "--from", base.Add(-time.Minute).Format(time.RFC3339), "--to", base.Add(3 * time.Hour).Format(time.RFC3339), "--limit", "10", "--max-tokens", "100", "--db", dbPath, "--output", "json", "editor"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("search = %d, stderr = %q", code, stderr.String())
		}
		var page activity.SearchPage
		if err := json.Unmarshal(stdout.Bytes(), &page); err != nil {
			t.Fatalf("decode search JSON: %v (%q)", err, stdout.String())
		}
		if len(page.Results) != 1 || page.Results[0].ID != "cli-segment" {
			t.Fatalf("search page = %+v", page)
		}
		if !strings.HasPrefix(page.Results[0].Summary, cliActivityFenceStart+"\n") || !strings.HasSuffix(page.Results[0].Summary, "\n"+cliActivityFenceEnd) {
			t.Fatalf("summary is not fenced: %q", page.Results[0].Summary)
		}
		if page.Results[0].Tokens < 1 || page.UsedTokens != page.Results[0].Tokens {
			t.Fatalf("token accounting = %+v", page)
		}
	})

	t.Run("get table", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"activity", "get", "--profile", "activity", "--max-tokens", "100", "--db", dbPath, "cli-segment"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("get = %d, stderr = %q", code, stderr.String())
		}
		if !strings.Contains(stdout.String(), base.Format(time.RFC3339)+"	segment	") || !strings.Contains(stdout.String(), cliActivityFenceStart) {
			t.Fatalf("unexpected get table: %q", stdout.String())
		}
	})

	t.Run("status json", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"activity", "status", "--profile", "activity", "--max-tokens", "100", "--db", dbPath, "--json"}, &stdout, &stderr)
		if code != exitcodes.ExitOK {
			t.Fatalf("status = %d, stderr = %q", code, stderr.String())
		}
		var status activity.Status
		if err := json.Unmarshal(stdout.Bytes(), &status); err != nil {
			t.Fatalf("decode status JSON: %v (%q)", err, stdout.String())
		}
		if status.ActiveSegments != 1 || status.ActiveEpisodes != 1 || status.Earliest == nil || status.Latest == nil {
			t.Fatalf("status = %+v", status)
		}
	})

	t.Run("denied profile", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"activity", "status", "--profile", "restricted", "--max-tokens", "100", "--db", dbPath}, &stdout, &stderr)
		if code != exitcodes.ExitNoInput || !strings.Contains(stderr.String(), "explicitly expose activity read tools") {
			t.Fatalf("denied profile = %d, stdout=%q stderr=%q", code, stdout.String(), stderr.String())
		}
	})
}

func TestCmdActivity_ValidationAndHelpPaths(t *testing.T) {
	home := sandboxHome(t)
	writeActivityReadProfile(t, home, "activity", true)
	cases := []struct {
		name string
		args []string
		want string
	}{
		{name: "help", args: []string{"activity", "--help"}, want: "bounded, profile-gated activity reads"},
		{name: "unknown", args: []string{"activity", "bogus", "--profile", "activity"}, want: "unknown subcommand"},
		{name: "missing profile", args: []string{"activity", "status", "--max-tokens", "10"}, want: "explicitly expose activity read tools"},
		{name: "invalid bounds", args: []string{"activity", "search", "--profile", "activity", "--from", "2026-08-28T12:00:00Z", "--to", "2026-08-28T13:00:00Z", "--limit", "0", "--max-tokens", "10", "editor"}, want: "invalid bounds"},
		{name: "invalid window", args: []string{"activity", "search", "--profile", "activity", "--from", "not-a-time", "--to", "2026-08-28T13:00:00Z", "--limit", "1", "--max-tokens", "10", "editor"}, want: "from must be RFC3339"},
		{name: "missing item", args: []string{"activity", "get", "--profile", "activity", "--max-tokens", "10", "--db", t.TempDir() + "/missing.db", "missing"}, want: "activity not found"},
	}
	for _, tt := range cases {
		t.Run(tt.name, func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := run(tt.args, &stdout, &stderr)
			if tt.name == "help" {
				if code != exitcodes.ExitOK || !strings.Contains(stdout.String(), tt.want) {
					t.Fatalf("help = %d, stdout=%q stderr=%q", code, stdout.String(), stderr.String())
				}
				return
			}
			if code == exitcodes.ExitOK || !strings.Contains(stderr.String(), tt.want) {
				t.Fatalf("validation = %d, stdout=%q stderr=%q, want %q", code, stdout.String(), stderr.String(), tt.want)
			}
		})
	}
}

func TestCLIActivityHelpersFenceUnicodeAndParseWindow(t *testing.T) {
	if got := fenceCLIActivitySummary("abcdef", 1); got != cliActivityFenceStart+"\n"+""+"\n"+cliActivityFenceEnd {
		t.Fatalf("fenced short budget = %q", got)
	}
	if got := fenceCLIActivitySummary("äöü", 2); strings.Contains(got, "ä") || !strings.HasSuffix(got, "\n"+cliActivityFenceEnd) {
		t.Fatalf("fenced unicode summary = %q", got)
	}
	if got := cliActivityTokenCount(""); got != 0 {
		t.Fatalf("empty token count = %d", got)
	}
	if got := cliActivityTokenCount("abcd"); got != 2 {
		t.Fatalf("token count = %d, want 2", got)
	}
	from, to, err := parseCLIActivityWindow("2026-08-28T12:00:00+02:00", "2026-08-28T13:00:00+02:00")
	if err != nil || from.Location() != time.UTC || to.Sub(from) != time.Hour {
		t.Fatalf("parsed window = %v, %v, err=%v", from, to, err)
	}
	if _, _, err := parseCLIActivityWindow("bad", "bad"); err == nil {
		t.Fatal("invalid from time unexpectedly accepted")
	}
}
