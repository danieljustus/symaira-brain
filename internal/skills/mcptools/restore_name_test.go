package mcptools

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

func TestSkillsRestoreRejectsUnsafeNames(t *testing.T) {
	for _, name := range []string{"", ".", "..", "../../../../victim", "a/b", `a\b`, "/victim", "a\x00b"} {
		for _, dryRun := range []bool{true, false} {
			t.Run(fmt.Sprintf("%q/dry_run=%t", name, dryRun), func(t *testing.T) {
				root := t.TempDir()
				// Four parents from either the library or the random restore
				// directory land at root, keeping even the vulnerable test safe.
				lib := filepath.Join(root, "l", "m", "n", "o")
				tmp := filepath.Join(root, "a", "b", "c")
				for _, dir := range []string{lib, tmp} {
					if err := os.MkdirAll(dir, 0o755); err != nil {
						t.Fatal(err)
					}
				}
				t.Setenv("TMPDIR", tmp)
				t.Setenv("TMP", tmp)
				t.Setenv("TEMP", tmp)
				src := t.TempDir()
				writeTestSkillContent(t, src, "victim", "v1 body")
				if _, err := skill.ImportSkillOpts(src, root, skill.ImportOptions{VCSEnabled: true}); err != nil {
					t.Fatal(err)
				}
				victim := filepath.Join(root, "victim")
				writeTestSkillContent(t, victim, "victim", "v2 body")
				if _, err := vcs.Commit(victim, "update: v2"); err != nil {
					t.Fatal(err)
				}
				before, err := os.ReadFile(filepath.Join(victim, "SKILL.md"))
				if err != nil {
					t.Fatal(err)
				}
				head := gitHead(t, victim)
				srv := mcpserver.New("symskills", "test")
				Register(srv, Options{LibraryDir: lib, HomeDir: root})
				args, err := json.Marshal(map[string]any{"name": name, "rev": "HEAD~1", "dry_run": dryRun})
				if err != nil {
					t.Fatal(err)
				}
				text, isErr := callTool(t, srv, "skills_restore", string(args))
				want := "restore skill: name and rev are required"
				if name != "" {
					want = "restore skill: " + skill.ValidateSkillName(name).Error()
				}
				if !isErr || text != want {
					t.Errorf("got (%q, %t), want validation error %q", text, isErr, want)
				}
				after, err := os.ReadFile(filepath.Join(victim, "SKILL.md"))
				if err != nil {
					t.Fatal(err)
				}
				if string(after) != string(before) || gitHead(t, victim) != head {
					t.Error("restore modified the victim outside the extraction temp directory")
				}
				entries, err := os.ReadDir(tmp)
				if err != nil {
					t.Fatal(err)
				}
				if len(entries) != 0 {
					t.Errorf("restore left temporary files: %v", entries)
				}
			})
		}
	}
}
