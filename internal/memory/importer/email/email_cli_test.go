package email

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// writeFakeHimalaya installs a fake `himalaya` executable on PATH. The fake
// dispatches on subcommand and emits JSON from fixture files referenced by
// env vars, so every DiscoverSessions/ImportSession branch (success, parse
// error, CLI failure) is testable without the real binary or any network.
func writeFakeHimalaya(t *testing.T) {
	t.Helper()
	dir := t.TempDir()
	script := `#!/bin/sh
if [ -n "$FAKE_HIMALAYA_FAIL" ]; then
  echo "fake himalaya failure" >&2
  exit 1
fi
case "$1" in
  message)
    if [ "$2" = "list" ]; then
      [ -n "$FAKE_HIMALAYA_ARGS" ] && echo "$@" > "$FAKE_HIMALAYA_ARGS"
      cat "$FAKE_HIMALAYA_LIST" 2>/dev/null || echo '[]'
    elif [ "$2" = "get" ]; then
      [ -n "$FAKE_HIMALAYA_ARGS" ] && echo "$@" > "$FAKE_HIMALAYA_ARGS"
      cat "$FAKE_HIMALAYA_GET" 2>/dev/null || echo '{"text":""}'
    else
      echo "unknown subcommand" >&2
      exit 1
    fi
    ;;
  *)
    echo "unknown command" >&2
    exit 1
    ;;
esac
`
	bin := filepath.Join(dir, "himalaya")
	if err := os.WriteFile(bin, []byte(script), 0o755); err != nil {
		t.Fatalf("write fake himalaya: %v", err)
	}
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
}

// writeFixture writes content to a temp file and returns its path.
func writeFixture(t *testing.T, content string) string {
	t.Helper()
	p := filepath.Join(t.TempDir(), "fixture.json")
	if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
	return p
}

func TestDiscoverSessionsParsesMessages(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `[
		{"id":"msg1","subject":"Old","from":"a@example.com","date":"2026-06-01T10:00:00Z","folder":"INBOX"},
		{"id":"msg2","subject":"Bad Date","from":"b@example.com","date":"not-a-date","folder":"INBOX"},
		{"id":"msg3","subject":"Recent","from":"c@example.com","date":"2026-07-15T10:00:00Z","folder":"INBOX","flags":["\\Seen"]}
	]`))

	imp := NewEmailImporter("INBOX", "", 2000)
	since := time.Date(2026, 7, 1, 0, 0, 0, 0, time.UTC)
	sessions, err := imp.DiscoverSessions(since)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (old + unparseable dates filtered), got %d", len(sessions))
	}
	s := sessions[0]
	if s.Tool != "email" {
		t.Errorf("Tool = %q, want %q", s.Tool, "email")
	}
	if s.SessionID != "msg3" {
		t.Errorf("SessionID = %q, want %q", s.SessionID, "msg3")
	}
	if s.Path != "email://INBOX/msg3" {
		t.Errorf("Path = %q, want %q", s.Path, "email://INBOX/msg3")
	}
	if s.Metadata["subject"] != "Recent" || s.Metadata["from"] != "c@example.com" || s.Metadata["folder"] != "INBOX" {
		t.Errorf("unexpected metadata: %v", s.Metadata)
	}
	if want := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC); !s.ModifiedAt.Equal(want) {
		t.Errorf("ModifiedAt = %v, want %v", s.ModifiedAt, want)
	}
}

func TestDiscoverSessionsEmptyList(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `[]`))

	imp := NewEmailImporter("INBOX", "", 2000)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 0 {
		t.Errorf("expected 0 sessions, got %d", len(sessions))
	}
}

func TestDiscoverSessionsParseError(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `this is not json`))

	imp := NewEmailImporter("INBOX", "", 2000)
	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error for malformed himalaya output")
	}
	if !strings.Contains(err.Error(), "failed to parse himalaya output") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestDiscoverSessionsCLIFailure(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_FAIL", "1")

	imp := NewEmailImporter("INBOX", "", 2000)
	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error when himalaya exits non-zero")
	}
	if !strings.Contains(err.Error(), "himalaya list failed") {
		t.Errorf("expected himalaya list failure, got %q", err.Error())
	}
}

func TestDiscoverSessionsImportanceFilter(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `[
		{"id":"m1","subject":"Flagged","from":"a@example.com","date":"2026-07-15T10:00:00Z","flags":["\\Flagged","high"]},
		{"id":"m2","subject":"Plain","from":"b@example.com","date":"2026-07-16T10:00:00Z","flags":["\\Seen"]}
	]`))

	tests := []struct {
		name       string
		importance string
		wantIDs    []string
	}{
		{"high matches flagged only", "high", []string{"m1"}},
		{"no match returns empty", "urgent", nil},
		{"no filter returns all", "", []string{"m1", "m2"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			imp := NewEmailImporter("INBOX", tt.importance, 2000)
			sessions, err := imp.DiscoverSessions(time.Time{})
			if err != nil {
				t.Fatalf("DiscoverSessions failed: %v", err)
			}
			var got []string
			for _, s := range sessions {
				got = append(got, s.SessionID)
			}
			if len(got) != len(tt.wantIDs) {
				t.Fatalf("got sessions %v, want %v", got, tt.wantIDs)
			}
			for i := range got {
				if got[i] != tt.wantIDs[i] {
					t.Errorf("session[%d] = %q, want %q", i, got[i], tt.wantIDs[i])
				}
			}
		})
	}
}

func TestDiscoverSessionsSenderFilters(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `[
		{"id":"m1","subject":"Work","from":"work@example.com","date":"2026-07-15T10:00:00Z"},
		{"id":"m2","subject":"Spam","from":"spam@example.com","date":"2026-07-16T10:00:00Z"}
	]`))

	tests := []struct {
		name           string
		includeSenders []string
		excludeSenders []string
		wantIDs        []string
	}{
		{"include keeps matching", []string{"work@example.com"}, nil, []string{"m1"}},
		{"include with no match drops all", []string{"nobody@example.com"}, nil, nil},
		{"exclude drops matching", nil, []string{"spam@example.com"}, []string{"m1"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			imp := NewEmailImporter("INBOX", "", 2000)
			imp.includeSenders = tt.includeSenders
			imp.excludeSenders = tt.excludeSenders
			sessions, err := imp.DiscoverSessions(time.Time{})
			if err != nil {
				t.Fatalf("DiscoverSessions failed: %v", err)
			}
			var got []string
			for _, s := range sessions {
				got = append(got, s.SessionID)
			}
			if len(got) != len(tt.wantIDs) {
				t.Fatalf("got sessions %v, want %v", got, tt.wantIDs)
			}
			for i := range got {
				if got[i] != tt.wantIDs[i] {
					t.Errorf("session[%d] = %q, want %q", i, got[i], tt.wantIDs[i])
				}
			}
		})
	}
}

func TestDiscoverSessionsEmptyFolderDefaultsToINBOXArgs(t *testing.T) {
	writeFakeHimalaya(t)
	argsFile := filepath.Join(t.TempDir(), "args.txt")
	t.Setenv("FAKE_HIMALAYA_ARGS", argsFile)
	t.Setenv("FAKE_HIMALAYA_LIST", writeFixture(t, `[{"id":"m1","subject":"S","from":"a@example.com","date":"2026-07-15T10:00:00Z"}]`))

	imp := NewEmailImporter("", "", 2000)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}
	if got := sessions[0].Metadata["folder"]; got != "INBOX" {
		t.Errorf("folder metadata = %q, want INBOX", got)
	}
	args, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatalf("read fake args: %v", err)
	}
	if !strings.Contains(string(args), "--folder INBOX") {
		t.Errorf("himalaya was not called with --folder INBOX, args: %q", string(args))
	}
}

func TestImportSessionParsesBody(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_GET", writeFixture(t, `{"text":"hello body"}`))

	imp := NewEmailImporter("INBOX", "", 2000)
	ref := importer.SessionRef{
		SessionID: "msg-1",
		Metadata: map[string]string{
			"folder":  "INBOX",
			"subject": "Test Subject",
			"from":    "sender@example.com",
			"date":    "2026-07-01T10:00:00Z",
		},
	}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	f := facts[0]
	if f.Source != "email" || f.SessionID != "msg-1" {
		t.Errorf("unexpected fact identity: source=%q sessionID=%q", f.Source, f.SessionID)
	}
	wantContent := "Email from sender@example.com: Test Subject\n\nhello body"
	if f.Content != wantContent {
		t.Errorf("Content = %q, want %q", f.Content, wantContent)
	}
	if f.Metadata["message_id"] != "msg-1" || f.Metadata["folder"] != "INBOX" || f.Metadata["source"] != "email" {
		t.Errorf("unexpected metadata: %v", f.Metadata)
	}
	if want := time.Date(2026, 7, 1, 10, 0, 0, 0, time.UTC); !f.Timestamp.Equal(want) {
		t.Errorf("Timestamp = %v, want %v", f.Timestamp, want)
	}
}

func TestImportSessionEmptyContent(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_GET", writeFixture(t, `{"text":""}`))

	imp := NewEmailImporter("INBOX", "", 2000)
	ref := importer.SessionRef{
		SessionID: "msg-empty",
		Metadata: map[string]string{
			"folder":  "INBOX",
			"subject": "No Body",
			"from":    "a@example.com",
		},
	}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if want := "Email from a@example.com: No Body"; facts[0].Content != want {
		t.Errorf("Content = %q, want %q", facts[0].Content, want)
	}
}

func TestImportSessionTruncatesBody(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_GET", writeFixture(t, `{"text":"0123456789ABCDEF"}`))

	imp := NewEmailImporter("INBOX", "", 10)
	ref := importer.SessionRef{
		SessionID: "msg-long",
		Metadata: map[string]string{
			"folder":  "INBOX",
			"subject": "Long",
			"from":    "a@example.com",
		},
	}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if !strings.Contains(facts[0].Content, "0123456789...") {
		t.Errorf("expected truncated body in content, got %q", facts[0].Content)
	}
}

func TestImportSessionParseError(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_GET", writeFixture(t, `not json at all`))

	imp := NewEmailImporter("INBOX", "", 2000)
	ref := importer.SessionRef{SessionID: "msg-bad", Metadata: map[string]string{"folder": "INBOX"}}
	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for malformed body JSON")
	}
	if !strings.Contains(err.Error(), "failed to parse himalaya body") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestImportSessionCLIFailure(t *testing.T) {
	writeFakeHimalaya(t)
	t.Setenv("FAKE_HIMALAYA_FAIL", "1")

	imp := NewEmailImporter("INBOX", "", 2000)
	ref := importer.SessionRef{SessionID: "msg-fail", Metadata: map[string]string{"folder": "INBOX"}}
	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error when himalaya exits non-zero")
	}
	if !strings.Contains(err.Error(), "himalaya get failed") {
		t.Errorf("expected himalaya get failure, got %q", err.Error())
	}
}

func TestImportSessionEmptyFolderDefaultsToINBOXArgs(t *testing.T) {
	writeFakeHimalaya(t)
	argsFile := filepath.Join(t.TempDir(), "args.txt")
	t.Setenv("FAKE_HIMALAYA_ARGS", argsFile)
	t.Setenv("FAKE_HIMALAYA_GET", writeFixture(t, `{"text":"body"}`))

	imp := NewEmailImporter("", "", 2000)
	ref := importer.SessionRef{
		SessionID: "msg-nofolder",
		Metadata: map[string]string{
			"subject": "No Folder",
			"from":    "a@example.com",
		},
	}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if got := facts[0].Metadata["folder"]; got != "INBOX" {
		t.Errorf("folder metadata = %q, want INBOX", got)
	}
	args, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatalf("read fake args: %v", err)
	}
	if !strings.Contains(string(args), "--folder INBOX") {
		t.Errorf("himalaya was not called with --folder INBOX, args: %q", string(args))
	}
}
