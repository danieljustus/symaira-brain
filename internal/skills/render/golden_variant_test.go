package render

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// TestGoldenVariantRender pins the per-target resolution of blocks, terms and
// only/except regions as committed files. The unit tests prove the mechanics;
// this proves what a reviewer would actually receive, in a form where a change
// to any harness's output shows up as a reviewable diff.
//
// Regenerate with:
//
//	go test ./internal/render/... -run TestGoldenVariantRender -update
//
// builtInTargets pins the golden set to the six shipped targets rather than
// reading the registry, which other tests append user-defined targets to.
var builtInTargets = []Target{
	TargetOpenCode, TargetClaude, TargetCodex,
	TargetHermes, TargetAntigravity, TargetOpenClaw,
}

func TestGoldenVariantRender(t *testing.T) {
	fixtureRoot := filepath.Join("testdata", "variant-source")
	bundle, err := skill.LoadBundle(fixtureRoot)
	if err != nil {
		t.Fatalf("LoadBundle(%s): %v", fixtureRoot, err)
	}

	for _, target := range builtInTargets {
		t.Run(string(target), func(t *testing.T) {
			rendered, err := RenderTarget(bundle, target)
			if err != nil {
				t.Fatalf("RenderTarget(%s): %v", target, err)
			}
			outDir := t.TempDir()
			if err := writeRendered(bundle.Root, outDir, rendered, target, sourceTreeHash(bundle.Root)); err != nil {
				t.Fatalf("writeRendered(%s): %v", target, err)
			}

			goldenDir := filepath.Join("testdata", "variant-golden", string(target))
			if *update {
				if err := os.RemoveAll(goldenDir); err != nil {
					t.Fatalf("RemoveAll(%s): %v", goldenDir, err)
				}
				if err := copyDir(outDir, goldenDir); err != nil {
					t.Fatalf("copy golden: %v", err)
				}
				return
			}

			golden, err := loadTree(goldenDir)
			if err != nil {
				t.Fatalf("load golden tree %s: %v", goldenDir, err)
			}
			actual, err := loadTree(outDir)
			if err != nil {
				t.Fatalf("load output tree %s: %v", outDir, err)
			}
			for rel, want := range golden {
				got, ok := actual[rel]
				if !ok {
					t.Errorf("golden file missing from output: %s", rel)
					continue
				}
				if want != got {
					t.Errorf("output differs for %s:\n-want: %q\n+got:  %q", rel, want, got)
				}
			}
			for rel := range actual {
				if _, ok := golden[rel]; !ok {
					t.Errorf("extra file in output not in golden: %s", rel)
				}
			}
		})
	}
}

// TestGoldenVariantInvariants asserts the properties that must hold for every
// target regardless of what the goldens happen to contain, so a careless
// `-update` cannot quietly bless broken output.
func TestGoldenVariantInvariants(t *testing.T) {
	for _, target := range builtInTargets {
		dir := filepath.Join("testdata", "variant-golden", string(target))
		tree, err := loadTree(dir)
		if err != nil {
			t.Fatalf("load golden tree %s: %v", dir, err)
		}
		for rel, content := range tree {
			if !strings.HasSuffix(rel, ".md") {
				continue
			}
			if strings.Contains(content, "symskills:") {
				t.Errorf("%s/%s: variant markers must never reach a rendered skill", target, rel)
			}
			if strings.Contains(content, "{{term:") {
				t.Errorf("%s/%s: unresolved term placeholder in rendered output", target, rel)
			}
		}
		if _, ok := tree["overlays/claude/blocks/dispatch.md"]; ok {
			t.Errorf("%s: overlay input must not be shipped", target)
		}

		skillMD := tree["SKILL.md"]
		hermesOnly := "Provider-backed children are allowed"
		if target == TargetHermes {
			if !strings.Contains(skillMD, hermesOnly) {
				t.Errorf("hermes: only-region missing from its own render")
			}
			if !strings.Contains(skillMD, "~/.hermes/reports/variant-fixture/") {
				t.Errorf("hermes: term override missing")
			}
			if strings.Contains(skillMD, "Use whatever isolation mechanism") {
				t.Errorf("hermes: except-region must be dropped for the listed target")
			}
			continue
		}
		if strings.Contains(skillMD, hermesOnly) {
			t.Errorf("%s: hermes-only region leaked into another target", target)
		}
		if !strings.Contains(skillMD, "~/.local/state/symskills/reports/variant-fixture/") {
			t.Errorf("%s: default term value missing", target)
		}
		if !strings.Contains(skillMD, "Use whatever isolation mechanism") {
			t.Errorf("%s: except-region must survive for unlisted targets", target)
		}
	}
}
