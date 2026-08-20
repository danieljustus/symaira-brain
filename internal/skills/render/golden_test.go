package render

import (
	"flag"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// TestGoldenRender asserts that rendering the canonical fixture bundle for
// each target produces output identical to the committed golden tree, and
// that the resolved install path matches the harness contract.
//
// Pass -update to regenerate golden files:
//
//	go test ./internal/render/... -run TestGoldenRender -update
var update = flag.Bool("update", false, "update golden files")

// expectedInstallPath returns the global-scope install path suffix for a
// target and skill name.  This mirrors the contract in internal/install.
func expectedInstallPath(target Target, name string) string {
	switch target {
	case TargetOpenCode:
		return ".config/opencode/skills/" + name
	case TargetClaude:
		return ".claude/skills/" + name
	case TargetCodex:
		return ".agents/skills/" + name
	case TargetHermes:
		return ".hermes/skills/symaira/" + name
	case TargetAntigravity:
		return ".gemini/antigravity-cli/skills/" + name
	case TargetOpenClaw:
		return ".openclaw/skills/" + name
	default:
		panic("unknown target: " + string(target))
	}
}

// expectedProjectInstallPath returns the project-scope install path suffix.
func expectedProjectInstallPath(target Target, name string) string {
	switch target {
	case TargetOpenCode:
		return ".opencode/skills/" + name
	case TargetClaude:
		return ".claude/skills/" + name
	case TargetCodex:
		return ".agents/skills/" + name
	case TargetHermes:
		return ".hermes/skills/" + name
	case TargetAntigravity:
		return ".agents/skills/" + name
	case TargetOpenClaw:
		return ".agents/skills/" + name
	default:
		panic("unknown target: " + string(target))
	}
}

func TestGoldenRender(t *testing.T) {
	fixtureRoot := filepath.Join("testdata", "source")

	// Load the canonical fixture bundle.
	bundle, err := skill.LoadBundle(fixtureRoot)
	if err != nil {
		t.Fatalf("LoadBundle(%s): %v", fixtureRoot, err)
	}

	type goldenCase struct {
		target          Target
		expectedName    string
		goldenDir       string
		expectedGlobal  string // expected global install path suffix
		expectedProject string // expected project-scope install path suffix
	}

	cases := []goldenCase{
		{
			target:          TargetOpenCode,
			expectedName:    "golden-fixture-open",
			goldenDir:       filepath.Join("testdata", "golden", "opencode"),
			expectedGlobal:  ".config/opencode/skills/golden-fixture-open",
			expectedProject: ".opencode/skills/golden-fixture-open",
		},
		{
			target:          TargetClaude,
			expectedName:    "golden-fixture",
			goldenDir:       filepath.Join("testdata", "golden", "claude"),
			expectedGlobal:  ".claude/skills/golden-fixture",
			expectedProject: ".claude/skills/golden-fixture",
		},
		{
			target:          TargetCodex,
			expectedName:    "golden-fixture-codex",
			goldenDir:       filepath.Join("testdata", "golden", "codex"),
			expectedGlobal:  ".agents/skills/golden-fixture-codex",
			expectedProject: ".agents/skills/golden-fixture-codex",
		},
		{
			target:          TargetHermes,
			expectedName:    "golden-fixture",
			goldenDir:       filepath.Join("testdata", "golden", "hermes"),
			expectedGlobal:  ".hermes/skills/symaira/golden-fixture",
			expectedProject: ".hermes/skills/golden-fixture",
		},
		{
			target:          TargetAntigravity,
			expectedName:    "golden-fixture",
			goldenDir:       filepath.Join("testdata", "golden", "antigravity"),
			expectedGlobal:  ".gemini/antigravity-cli/skills/golden-fixture",
			expectedProject: ".agents/skills/golden-fixture",
		},
		{
			target:          TargetOpenClaw,
			expectedName:    "golden-fixture",
			goldenDir:       filepath.Join("testdata", "golden", "openclaw"),
			expectedGlobal:  ".openclaw/skills/golden-fixture",
			expectedProject: ".agents/skills/golden-fixture",
		},
	}

	for _, c := range cases {
		t.Run(string(c.target), func(t *testing.T) {
			// --- Render ---
			rendered, err := RenderTarget(bundle, c.target)
			if err != nil {
				t.Fatalf("RenderTarget(%s): %v", c.target, err)
			}

			// Assert resolved name.
			if rendered.Name != c.expectedName {
				t.Errorf("rendered name: want %q, got %q", c.expectedName, rendered.Name)
			}

			// Write rendered output to a temp directory so we can compare
			// the full filesystem tree (SKILL.md, support files, metadata).
			outDir := t.TempDir()
			if err := writeRendered(bundle.Root, outDir, rendered, c.target, sourceTreeHash(bundle.Root)); err != nil {
				t.Fatalf("writeRendered(%s): %v", c.target, err)
			}

			// --- Install path assertions ---
			t.Run("install_path", func(t *testing.T) {
				gotGlobal := expectedInstallPath(c.target, rendered.Name)
				if gotGlobal != c.expectedGlobal {
					t.Errorf("global install path: want %q, got %q", c.expectedGlobal, gotGlobal)
				}

				gotProject := expectedProjectInstallPath(c.target, rendered.Name)
				if gotProject != c.expectedProject {
					t.Errorf("project install path: want %q, got %q", c.expectedProject, gotProject)
				}
			})

			// --- Golden comparison ---
			t.Run("golden", func(t *testing.T) {
				if *update {
					// Regenerate: replace golden dir with current output.
					if err := os.RemoveAll(c.goldenDir); err != nil {
						t.Fatalf("RemoveAll(%s): %v", c.goldenDir, err)
					}
					if err := copyDir(outDir, c.goldenDir); err != nil {
						t.Fatalf("copy golden: %v", err)
					}
					return
				}

				// Load golden tree.
				golden, err := loadTree(c.goldenDir)
				if err != nil {
					t.Fatalf("load golden tree %s: %v", c.goldenDir, err)
				}

				// Load actual output tree.
				actual, err := loadTree(outDir)
				if err != nil {
					t.Fatalf("load output tree %s: %v", outDir, err)
				}

				// Compare files.
				for rel, wantContent := range golden {
					gotContent, ok := actual[rel]
					if !ok {
						t.Errorf("golden file missing from output: %s", rel)
						continue
					}
					if wantContent != gotContent {
						t.Errorf("output differs for %s:\n--- want (golden)\n+++ got (actual)\n-want: %q\n+got:  %q",
							rel, wantContent, gotContent)
					}
				}
				// Report extra files in output not present in golden.
				for rel := range actual {
					if _, ok := golden[rel]; !ok {
						t.Errorf("extra file in output not in golden: %s", rel)
					}
				}
			})
		})
	}
}

// loadTree walks dir and returns a map of relative file paths to their content.
func loadTree(dir string) (map[string]string, error) {
	tree := map[string]string{}
	err := filepath.WalkDir(dir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(dir, path)
		if err != nil {
			return err
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		tree[rel] = string(data)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return tree, nil
}

// copyDir recursively copies src into dst (dst is created if needed).
func copyDir(src, dst string) error {
	return filepath.WalkDir(src, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		dstPath := filepath.Join(dst, rel)
		if d.IsDir() {
			return os.MkdirAll(dstPath, 0o755)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return os.WriteFile(dstPath, data, 0o644)
	})
}
