package github

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// writeFakeGH installs a fake `gh` executable on PATH. The fake dispatches on
// the subcommand group ($1_$2: pr_list, pr_view, issue_list, issue_view) and
// emits JSON from fixture files referenced by env vars, so every
// discovery/import branch (success, parse error, CLI failure) is testable
// without the real binary or any network.
func writeFakeGH(t *testing.T) {
	t.Helper()
	dir := t.TempDir()
	script := `#!/bin/sh
if [ -n "$FAKE_GH_FAIL" ]; then
  echo "fake gh failure" >&2
  exit 1
fi
cmd="$1_$2"
case "$cmd" in
  pr_list)
    if [ -n "$FAKE_GH_FAIL_PR_LIST" ]; then
      echo "fake gh pr list failure" >&2
      exit 1
    fi
    [ -n "$FAKE_GH_ARGS" ] && echo "$@" > "$FAKE_GH_ARGS"
    cat "$FAKE_GH_PR_LIST" 2>/dev/null || echo '[]'
    ;;
  issue_list)
    if [ -n "$FAKE_GH_FAIL_ISSUE_LIST" ]; then
      echo "fake gh issue list failure" >&2
      exit 1
    fi
    [ -n "$FAKE_GH_ARGS" ] && echo "$@" > "$FAKE_GH_ARGS"
    cat "$FAKE_GH_ISSUE_LIST" 2>/dev/null || echo '[]'
    ;;
  pr_view)
    [ -n "$FAKE_GH_ARGS" ] && echo "$@" > "$FAKE_GH_ARGS"
    cat "$FAKE_GH_PR_VIEW" 2>/dev/null || echo '{}'
    ;;
  issue_view)
    [ -n "$FAKE_GH_ARGS" ] && echo "$@" > "$FAKE_GH_ARGS"
    cat "$FAKE_GH_ISSUE_VIEW" 2>/dev/null || echo '{}'
    ;;
  *)
    echo "unknown gh command: $cmd" >&2
    exit 1
    ;;
esac
`
	bin := filepath.Join(dir, "gh")
	if err := os.WriteFile(bin, []byte(script), 0o755); err != nil {
		t.Fatalf("write fake gh: %v", err)
	}
	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))
}

func writeFixture(t *testing.T, content string) string {
	t.Helper()
	p := filepath.Join(t.TempDir(), "fixture.json")
	if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
	return p
}

func TestDiscoverPRsParsesOutput(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_LIST", writeFixture(t, `[
		{"number":1,"title":"Fix bug","body":"desc","state":"OPEN","author":"alice","createdAt":"2026-07-10T09:00:00Z","mergedAt":"","url":"https://github.com/o/r/pull/1","labels":["bug"]},
		{"number":2,"title":"Add feature","body":"","state":"MERGED","author":"bob","createdAt":"2026-07-11T09:00:00Z","mergedAt":"2026-07-12T09:00:00Z","url":"https://github.com/o/r/pull/2","labels":[]}
	]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.discoverPRs(time.Time{})
	if err != nil {
		t.Fatalf("discoverPRs failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}
	s := sessions[0]
	if s.Tool != "github" || s.SessionID != "pr/1" {
		t.Errorf("unexpected session: tool=%q id=%q", s.Tool, s.SessionID)
	}
	if s.Path != "https://github.com/o/r/pull/1" {
		t.Errorf("Path = %q, want pull URL", s.Path)
	}
	if s.Metadata["type"] != "pr" || s.Metadata["state"] != "OPEN" || s.Metadata["title"] != "Fix bug" || s.Metadata["author"] != "alice" {
		t.Errorf("unexpected metadata: %v", s.Metadata)
	}
	if want := time.Date(2026, 7, 10, 9, 0, 0, 0, time.UTC); !s.ModifiedAt.Equal(want) {
		t.Errorf("ModifiedAt = %v, want %v", s.ModifiedAt, want)
	}
}

func TestDiscoverPRsParseError(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_LIST", writeFixture(t, `not json`))

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.discoverPRs(time.Time{})
	if err == nil {
		t.Fatal("expected error for malformed PR JSON")
	}
	if !strings.Contains(err.Error(), "failed to parse PR JSON") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestDiscoverPRsCLIFailure(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL", "1")

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.discoverPRs(time.Time{})
	if err == nil {
		t.Fatal("expected error when gh exits non-zero")
	}
	if !strings.Contains(err.Error(), "gh pr list failed") {
		t.Errorf("expected gh pr list failure, got %q", err.Error())
	}
}

func TestDiscoverIssuesParsesOutput(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_LIST", writeFixture(t, `[
		{"number":10,"title":"Bug report","body":"details","state":"OPEN","author":"carol","createdAt":"2026-07-10T09:00:00Z","closedAt":"","url":"https://github.com/o/r/issues/10","labels":["bug"]},
		{"number":11,"title":"Question","body":"","state":"CLOSED","author":"dave","createdAt":"2026-07-11T09:00:00Z","closedAt":"2026-07-12T09:00:00Z","url":"https://github.com/o/r/issues/11","labels":[]}
	]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.discoverIssues(time.Time{})
	if err != nil {
		t.Fatalf("discoverIssues failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}
	s := sessions[0]
	if s.SessionID != "issue/10" {
		t.Errorf("SessionID = %q, want %q", s.SessionID, "issue/10")
	}
	if s.Path != "https://github.com/o/r/issues/10" {
		t.Errorf("Path = %q, want issue URL", s.Path)
	}
	if s.Metadata["type"] != "issue" || s.Metadata["state"] != "OPEN" || s.Metadata["title"] != "Bug report" || s.Metadata["author"] != "carol" {
		t.Errorf("unexpected metadata: %v", s.Metadata)
	}
}

func TestDiscoverIssuesEmptyList(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_LIST", writeFixture(t, `[]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.discoverIssues(time.Time{})
	if err != nil {
		t.Fatalf("discoverIssues failed: %v", err)
	}
	if len(sessions) != 0 {
		t.Errorf("expected 0 sessions, got %d", len(sessions))
	}
}

func TestDiscoverIssuesParseError(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_LIST", writeFixture(t, `not json`))

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.discoverIssues(time.Time{})
	if err == nil {
		t.Fatal("expected error for malformed issue JSON")
	}
	if !strings.Contains(err.Error(), "failed to parse issue JSON") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestDiscoverIssuesCLIFailure(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL", "1")

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.discoverIssues(time.Time{})
	if err == nil {
		t.Fatal("expected error when gh exits non-zero")
	}
	if !strings.Contains(err.Error(), "gh issue list failed") {
		t.Errorf("expected gh issue list failure, got %q", err.Error())
	}
}

func TestDiscoverSessionsMergesPRsAndIssues(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_LIST", writeFixture(t, `[
		{"number":1,"title":"PR","state":"OPEN","author":"a","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/pull/1"}
	]`))
	t.Setenv("FAKE_GH_ISSUE_LIST", writeFixture(t, `[
		{"number":2,"title":"Issue","state":"OPEN","author":"b","createdAt":"2026-07-11T09:00:00Z","url":"https://github.com/o/r/issues/2"}
	]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions (PR + issue), got %d", len(sessions))
	}
	if sessions[0].SessionID != "pr/1" || sessions[1].SessionID != "issue/2" {
		t.Errorf("unexpected session order/ids: %q, %q", sessions[0].SessionID, sessions[1].SessionID)
	}
}

func TestDiscoverSessionsPRFailureKeepsIssues(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL_PR_LIST", "1")
	t.Setenv("FAKE_GH_ISSUE_LIST", writeFixture(t, `[
		{"number":2,"title":"Issue","state":"OPEN","author":"b","createdAt":"2026-07-11T09:00:00Z","url":"https://github.com/o/r/issues/2"}
	]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should swallow PR discovery errors, got: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 issue session (PR discovery failed), got %d", len(sessions))
	}
	if sessions[0].SessionID != "issue/2" {
		t.Errorf("SessionID = %q, want %q", sessions[0].SessionID, "issue/2")
	}
}

func TestDiscoverSessionsIssueFailureKeepsPRs(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL_ISSUE_LIST", "1")
	t.Setenv("FAKE_GH_PR_LIST", writeFixture(t, `[
		{"number":1,"title":"PR","state":"OPEN","author":"a","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/pull/1"}
	]`))

	imp := NewGitHubImporter("o", "r", "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should swallow issue discovery errors, got: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 PR session (issue discovery failed), got %d", len(sessions))
	}
	if sessions[0].SessionID != "pr/1" {
		t.Errorf("SessionID = %q, want %q", sessions[0].SessionID, "pr/1")
	}
}

func TestImportSessionPRHappyPath(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_VIEW", writeFixture(t, `{
		"number":42,"title":"Cool PR","body":"PR body text","state":"OPEN","author":"alice",
		"createdAt":"2026-07-10T09:00:00Z","mergedAt":"","url":"https://github.com/o/r/pull/42","labels":["enhancement"]
	}`))

	imp := NewGitHubImporter("o", "r", "")
	ref := importer.SessionRef{SessionID: "pr/42"}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	f := facts[0]
	wantContent := "PR #42: Cool PR\n\nPR body text"
	if f.Content != wantContent {
		t.Errorf("Content = %q, want %q", f.Content, wantContent)
	}
	if f.Source != "github" || f.SessionID != "pr/42" {
		t.Errorf("unexpected identity: source=%q id=%q", f.Source, f.SessionID)
	}
	if f.Metadata["type"] != "pr" || f.Metadata["state"] != "OPEN" || f.Metadata["url"] != "https://github.com/o/r/pull/42" {
		t.Errorf("unexpected metadata: %v", f.Metadata)
	}
	if want := time.Date(2026, 7, 10, 9, 0, 0, 0, time.UTC); !f.Timestamp.Equal(want) {
		t.Errorf("Timestamp = %v, want %v", f.Timestamp, want)
	}
}

func TestImportSessionPRNoBody(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_VIEW", writeFixture(t, `{"number":7,"title":"No body PR","state":"OPEN","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/pull/7"}`))

	imp := NewGitHubImporter("o", "r", "")
	facts, err := imp.ImportSession(importer.SessionRef{SessionID: "pr/7"})
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if want := "PR #7: No body PR"; facts[0].Content != want {
		t.Errorf("Content = %q, want %q", facts[0].Content, want)
	}
}

func TestImportSessionPRBodyTruncation(t *testing.T) {
	writeFakeGH(t)
	longBody := strings.Repeat("x", 2500)
	t.Setenv("FAKE_GH_PR_VIEW", writeFixture(t, `{"number":9,"title":"Long","body":"`+longBody+`","state":"OPEN","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/pull/9"}`))

	imp := NewGitHubImporter("o", "r", "")
	facts, err := imp.ImportSession(importer.SessionRef{SessionID: "pr/9"})
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if !strings.Contains(facts[0].Content, strings.Repeat("x", 2000)+"...") {
		t.Errorf("expected truncated body (2000 chars + ...), got content of length %d", len(facts[0].Content))
	}
}

func TestImportSessionPRParseError(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_PR_VIEW", writeFixture(t, `not json`))

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.ImportSession(importer.SessionRef{SessionID: "pr/42"})
	if err == nil {
		t.Fatal("expected error for malformed PR view JSON")
	}
	if !strings.Contains(err.Error(), "failed to parse PR JSON") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestImportSessionPRCLIFailure(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL", "1")

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.ImportSession(importer.SessionRef{SessionID: "pr/42"})
	if err == nil {
		t.Fatal("expected error when gh pr view fails")
	}
	if !strings.Contains(err.Error(), "gh view failed for pr/42") {
		t.Errorf("expected gh view failure, got %q", err.Error())
	}
}

func TestImportSessionIssueHappyPath(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_VIEW", writeFixture(t, `{
		"number":7,"title":"Bug report","body":"issue body","state":"OPEN","author":"carol",
		"createdAt":"2026-07-10T09:00:00Z","closedAt":"","url":"https://github.com/o/r/issues/7","labels":["bug"]
	}`))

	imp := NewGitHubImporter("o", "r", "")
	ref := importer.SessionRef{SessionID: "issue/7"}
	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	f := facts[0]
	wantContent := "Issue #7: Bug report\n\nissue body"
	if f.Content != wantContent {
		t.Errorf("Content = %q, want %q", f.Content, wantContent)
	}
	if f.Metadata["type"] != "issue" || f.Metadata["state"] != "OPEN" || f.Metadata["url"] != "https://github.com/o/r/issues/7" {
		t.Errorf("unexpected metadata: %v", f.Metadata)
	}
}

func TestImportSessionIssueNoBody(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_VIEW", writeFixture(t, `{"number":8,"title":"No body issue","state":"OPEN","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/issues/8"}`))

	imp := NewGitHubImporter("o", "r", "")
	facts, err := imp.ImportSession(importer.SessionRef{SessionID: "issue/8"})
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if want := "Issue #8: No body issue"; facts[0].Content != want {
		t.Errorf("Content = %q, want %q", facts[0].Content, want)
	}
}

func TestImportSessionIssueBodyTruncation(t *testing.T) {
	writeFakeGH(t)
	longBody := strings.Repeat("y", 2500)
	t.Setenv("FAKE_GH_ISSUE_VIEW", writeFixture(t, `{"number":9,"title":"Long","body":"`+longBody+`","state":"OPEN","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/issues/9"}`))

	imp := NewGitHubImporter("o", "r", "")
	facts, err := imp.ImportSession(importer.SessionRef{SessionID: "issue/9"})
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if !strings.Contains(facts[0].Content, strings.Repeat("y", 2000)+"...") {
		t.Errorf("expected truncated body (2000 chars + ...), got content of length %d", len(facts[0].Content))
	}
}

func TestImportSessionIssueParseError(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_ISSUE_VIEW", writeFixture(t, `not json`))

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.ImportSession(importer.SessionRef{SessionID: "issue/7"})
	if err == nil {
		t.Fatal("expected error for malformed issue view JSON")
	}
	if !strings.Contains(err.Error(), "failed to parse issue JSON") {
		t.Errorf("expected parse error, got %q", err.Error())
	}
}

func TestImportSessionIssueCLIFailure(t *testing.T) {
	writeFakeGH(t)
	t.Setenv("FAKE_GH_FAIL", "1")

	imp := NewGitHubImporter("o", "r", "")
	_, err := imp.ImportSession(importer.SessionRef{SessionID: "issue/7"})
	if err == nil {
		t.Fatal("expected error when gh issue view fails")
	}
	if !strings.Contains(err.Error(), "gh view failed for issue/7") {
		t.Errorf("expected gh view failure, got %q", err.Error())
	}
}

func TestImportSessionUsesRepoFlag(t *testing.T) {
	writeFakeGH(t)
	argsFile := filepath.Join(t.TempDir(), "args.txt")
	t.Setenv("FAKE_GH_ARGS", argsFile)
	t.Setenv("FAKE_GH_PR_VIEW", writeFixture(t, `{"number":1,"title":"T","state":"OPEN","createdAt":"2026-07-10T09:00:00Z","url":"https://github.com/o/r/pull/1"}`))

	imp := NewGitHubImporter("myowner", "myrepo", "")
	if _, err := imp.ImportSession(importer.SessionRef{SessionID: "pr/1"}); err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	args, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatalf("read fake args: %v", err)
	}
	if !strings.Contains(string(args), "--repo myowner/myrepo") {
		t.Errorf("gh was not called with --repo myowner/myrepo, args: %q", string(args))
	}
}
