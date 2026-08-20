package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// gitLogCount returns the number of commits in the repo at dir.
func gitLogCount(t *testing.T, dir string) int {
	t.Helper()
	cmd := exec.Command("git", "log", "--format=%s")
	cmd.Dir = dir
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("git log in %s: %v", dir, err)
	}
	if strings.TrimSpace(string(out)) == "" {
		return 0
	}
	return len(strings.Split(strings.TrimSpace(string(out)), "\n"))
}

// gitCommit makes a hand-made commit in the repo at dir with the given
// subject (used to simulate user edits and invalid states).
func gitCommit(t *testing.T, dir, subject string) {
	t.Helper()
	cmd := exec.Command("git", "-c", "user.name=symskills", "-c", "user.email=symskills@localhost", "-c", "commit.gpgsign=false", "commit", "-am", subject)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git commit %q: %v: %s", subject, err, out)
	}
}

// gitHead returns the full HEAD hash of the repo at dir.
func gitHead(t *testing.T, dir string) string {
	t.Helper()
	cmd := exec.Command("git", "rev-parse", "HEAD")
	cmd.Dir = dir
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("git rev-parse: %v", err)
	}
	return strings.TrimSpace(string(out))
}

// newVersionedSkillServer builds an MCP server whose library contains a
// versioned skill: import v1 (initial commit), then update v2 (second
// commit). Returns the server, the library dir, the skill dir and the
// initial commit hash.
func newVersionedSkillServer(t *testing.T) (*mcpserver.Server, string, string, string) {
	t.Helper()
	home := t.TempDir()
	lib := filepath.Join(home, "library")
	if err := os.MkdirAll(lib, 0o755); err != nil {
		t.Fatal(err)
	}
	src := t.TempDir()
	writeTestSkillContent(t, src, "mcp-hist", "v1 body")
	if _, err := skill.ImportSkillOpts(src, lib, skill.ImportOptions{VCSEnabled: true}); err != nil {
		t.Fatalf("import: %v", err)
	}
	skillDir := filepath.Join(lib, "mcp-hist")
	first := gitHead(t, skillDir)
	writeTestSkillContent(t, src, "mcp-hist", "v2 body")
	if _, err := skill.ImportSkillOpts(src, lib, skill.ImportOptions{VCSEnabled: true, Update: true}); err != nil {
		t.Fatalf("update: %v", err)
	}
	srv := mcpserver.New("symskills", "test")
	Register(srv, Options{LibraryDir: lib, RenderDir: filepath.Join(home, "rendered"), HomeDir: home})
	return srv, lib, skillDir, first
}

// writeTestSkillContent writes a valid SKILL.md with the given name and a
// body marker.
func writeTestSkillContent(t *testing.T, dir, name, body string) {
	t.Helper()
	content := fmt.Sprintf("---\nname: %s\ndescription: %s\n---\n\n%s\n", name, name, body)
	if err := os.WriteFile(filepath.Join(dir, "SKILL.md"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

// callTool invokes one MCP tool and returns the raw JSON text payload and
// whether the call errored.
func callTool(t *testing.T, srv *mcpserver.Server, name, arguments string) (string, bool) {
	t.Helper()
	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":%q,"arguments":%s}}`+"\n", name, arguments)
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}
	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if resp.Error != nil {
		return resp.Error.Message, true
	}
	if len(resp.Result.Content) == 0 {
		t.Fatalf("no content in response: %q", out.String())
	}
	var text string
	if err := json.Unmarshal(resp.Result.Content[0].Text, &text); err != nil {
		t.Fatalf("unmarshal text: %v", err)
	}
	return text, resp.Result.IsError
}

func TestSkillsHistory(t *testing.T) {
	srv, _, _, first := newVersionedSkillServer(t)

	text, isErr := callTool(t, srv, "skills_history", `{"name":"mcp-hist"}`)
	if isErr {
		t.Fatalf("skills_history errored: %s", text)
	}
	var result struct {
		Name    string `json:"name"`
		History []struct {
			Revision  string   `json:"revision"`
			Timestamp string   `json:"timestamp"`
			Operation string   `json:"operation"`
			Subject   string   `json:"subject"`
			Files     []string `json:"files"`
		} `json:"history"`
	}
	if err := json.Unmarshal([]byte(text), &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if result.Name != "mcp-hist" {
		t.Errorf("expected name mcp-hist, got %q", result.Name)
	}
	if len(result.History) != 2 {
		t.Fatalf("expected 2 commits, got %d", len(result.History))
	}
	// Newest first: the update commit, then the import.
	if result.History[0].Operation != "update" {
		t.Errorf("expected newest operation update, got %q", result.History[0].Operation)
	}
	if result.History[1].Operation != "import" {
		t.Errorf("expected oldest operation import, got %q", result.History[1].Operation)
	}
	if result.History[1].Revision != first {
		t.Errorf("expected oldest revision %s, got %s", first, result.History[1].Revision)
	}
	if result.History[0].Timestamp == "" {
		t.Error("expected a timestamp")
	}
	if len(result.History[0].Files) != 1 || result.History[0].Files[0] != "SKILL.md" {
		t.Errorf("expected the update to touch SKILL.md, got %#v", result.History[0].Files)
	}
}

func TestSkillsHistoryLimitAndErrors(t *testing.T) {
	srv, _, _, _ := newVersionedSkillServer(t)

	text, isErr := callTool(t, srv, "skills_history", `{"name":"mcp-hist","limit":1}`)
	if isErr {
		t.Fatalf("skills_history errored: %s", text)
	}
	var result struct {
		History []json.RawMessage `json:"history"`
	}
	if err := json.Unmarshal([]byte(text), &result); err != nil {
		t.Fatal(err)
	}
	if len(result.History) != 1 {
		t.Fatalf("expected 1 commit with limit 1, got %d", len(result.History))
	}
	if _, isErr := callTool(t, srv, "skills_history", `{"name":"missing-skill"}`); !isErr {
		t.Fatal("expected an error for an unknown skill")
	}
	if _, isErr := callTool(t, srv, "skills_history", `{}`); !isErr {
		t.Fatal("expected an error when name is missing")
	}
}

// TestSkillsRestoreDryRunByDefault locks the #119 acceptance criterion:
// skills_restore defaults to dry-run and writes nothing.
func TestSkillsRestoreDryRunByDefault(t *testing.T) {
	srv, _, skillDir, first := newVersionedSkillServer(t)
	before := gitLogCount(t, skillDir)
	bodyBefore, err := os.ReadFile(filepath.Join(skillDir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}

	// No dry_run argument: must plan, not write.
	text, isErr := callTool(t, srv, "skills_restore", fmt.Sprintf(`{"name":"mcp-hist","rev":%q}`, first))
	if isErr {
		t.Fatalf("skills_restore errored: %s", text)
	}
	var result map[string]any
	if err := json.Unmarshal([]byte(text), &result); err != nil {
		t.Fatal(err)
	}
	if result["action"] != "planned" {
		t.Errorf("expected action planned, got %#v", result["action"])
	}
	if result["dry_run"] != true {
		t.Errorf("expected dry_run true by default, got %#v", result["dry_run"])
	}
	notes, ok := result["notes"].([]any)
	if !ok || len(notes) < 2 {
		t.Fatalf("expected restore guidance notes, got %#v", result["notes"])
	}
	if _, ok := result["commit"]; ok {
		t.Error("dry-run must not report a commit")
	}
	if after := gitLogCount(t, skillDir); after != before {
		t.Fatalf("dry-run wrote a commit: %d -> %d", before, after)
	}
	bodyAfter, err := os.ReadFile(filepath.Join(skillDir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if string(bodyAfter) != string(bodyBefore) {
		t.Fatal("dry-run modified the skill files")
	}
}

// TestSkillsRestoreApplies locks the #119 acceptance criterion: restoring
// returns the files to that state and leaves the intermediate history
// intact.
func TestSkillsRestoreApplies(t *testing.T) {
	srv, _, skillDir, first := newVersionedSkillServer(t)
	before := gitLogCount(t, skillDir)

	text, isErr := callTool(t, srv, "skills_restore", fmt.Sprintf(`{"name":"mcp-hist","rev":%q,"dry_run":false}`, first))
	if isErr {
		t.Fatalf("skills_restore errored: %s", text)
	}
	var result map[string]any
	if err := json.Unmarshal([]byte(text), &result); err != nil {
		t.Fatal(err)
	}
	if result["action"] != "restored" {
		t.Fatalf("expected action restored, got %#v", result["action"])
	}
	if result["commit"] == "" {
		t.Fatal("expected a commit hash")
	}
	if after := gitLogCount(t, skillDir); after != before+1 {
		t.Fatalf("expected one new commit, got %d -> %d", before, after)
	}
	data, err := os.ReadFile(filepath.Join(skillDir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "v1 body") {
		t.Errorf("expected v1 content after restore, got %q", data)
	}
	// Intermediate history intact: the update commit still carries v2.
	cmd := exec.Command("git", "show", "HEAD~1:SKILL.md")
	cmd.Dir = skillDir
	prev, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(prev), "v2 body") {
		t.Errorf("intermediate history lost v2: %q", prev)
	}
}

func TestSkillsRestoreRefusesInvalidState(t *testing.T) {
	srv, _, skillDir, _ := newVersionedSkillServer(t)
	// Hand-made commit with a broken SKILL.md.
	bad := "not a skill at all\n"
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte(bad), 0o644); err != nil {
		t.Fatal(err)
	}
	gitCommit(t, skillDir, "user: broken skill")

	text, isErr := callTool(t, srv, "skills_restore", fmt.Sprintf(`{"name":"mcp-hist","rev":%q,"dry_run":false}`, gitHead(t, skillDir)))
	if !isErr {
		t.Fatalf("expected refusal for an invalid restored state, got %s", text)
	}
	if !strings.Contains(text, "refusing restore") || !strings.Contains(text, "frontmatter") {
		t.Errorf("expected refusal naming the validation error, got %q", text)
	}
	// Nothing was written: the broken content is still on disk.
	data, err := os.ReadFile(filepath.Join(skillDir, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != bad {
		t.Fatal("refused restore must not modify the working tree")
	}
}

func TestSkillsRestoreRefusesDirtyWithoutAllowDirty(t *testing.T) {
	srv, _, skillDir, first := newVersionedSkillServer(t)
	// Uncommitted local edit.
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte("---\nname: mcp-hist\ndescription: mcp-hist\n---\n\nlocal edit\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	text, isErr := callTool(t, srv, "skills_restore", fmt.Sprintf(`{"name":"mcp-hist","rev":%q,"dry_run":false}`, first))
	if !isErr {
		t.Fatalf("expected refusal for a dirty tree, got %s", text)
	}
	if !strings.Contains(text, "uncommitted changes") || !strings.Contains(text, "allow_dirty") {
		t.Errorf("expected refusal naming uncommitted changes and allow_dirty, got %q", text)
	}
}

// TestSkillsRestoreAllowDirtySnapshots locks the #119 acceptance
// criterion: uncommitted changes are never lost without an explicit flag.
func TestSkillsRestoreAllowDirtySnapshots(t *testing.T) {
	srv, _, skillDir, first := newVersionedSkillServer(t)
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte("---\nname: mcp-hist\ndescription: mcp-hist\n---\n\nlocal edit\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	before := gitLogCount(t, skillDir)

	text, isErr := callTool(t, srv, "skills_restore", fmt.Sprintf(`{"name":"mcp-hist","rev":%q,"dry_run":false,"allow_dirty":true}`, first))
	if isErr {
		t.Fatalf("skills_restore errored: %s", text)
	}
	var result map[string]any
	if err := json.Unmarshal([]byte(text), &result); err != nil {
		t.Fatal(err)
	}
	if result["action"] != "restored" {
		t.Fatalf("expected action restored, got %#v", result["action"])
	}
	if after := gitLogCount(t, skillDir); after != before+2 {
		t.Fatalf("expected snapshot + restore commits, got %d -> %d", before, after)
	}
	// The snapshot commit holds the local edit: nothing was discarded.
	cmd := exec.Command("git", "show", "HEAD~1:SKILL.md")
	cmd.Dir = skillDir
	snapshot, err := cmd.Output()
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(snapshot), "local edit") {
		t.Errorf("uncommitted changes were lost: snapshot holds %q", snapshot)
	}
}

func TestSkillsRestoreNotVersioned(t *testing.T) {
	home := t.TempDir()
	lib := filepath.Join(home, "library")
	if err := os.MkdirAll(lib, 0o755); err != nil {
		t.Fatal(err)
	}
	plain := filepath.Join(lib, "plain-skill")
	if err := os.MkdirAll(plain, 0o755); err != nil {
		t.Fatal(err)
	}
	writeTestSkillContent(t, plain, "plain-skill", "unversioned")
	srv := mcpserver.New("symskills", "test")
	Register(srv, Options{LibraryDir: lib, HomeDir: home})

	text, isErr := callTool(t, srv, "skills_restore", `{"name":"plain-skill","rev":"HEAD"}`)
	if !isErr {
		t.Fatalf("expected an error for an unversioned skill, got %s", text)
	}
	if !strings.Contains(text, "not versioned") {
		t.Errorf("expected not-versioned error, got %q", text)
	}
	if _, isErr := callTool(t, srv, "skills_history", `{"name":"plain-skill"}`); !isErr {
		t.Fatal("expected skills_history to error for an unversioned skill")
	}
}

func TestSkillsRestoreUnknownRevision(t *testing.T) {
	srv, _, _, _ := newVersionedSkillServer(t)
	text, isErr := callTool(t, srv, "skills_restore", `{"name":"mcp-hist","rev":"nope"}`)
	if !isErr {
		t.Fatalf("expected an error for an unknown revision, got %s", text)
	}
	if !strings.Contains(text, "not found") {
		t.Errorf("expected not-found error, got %q", text)
	}
}
