package render

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// flowFixture returns the testdata path of a flow-skill fixture. The
// fixtures live in the top-level testdata directory so every package
// (skill, render, cmd/symskills) shares them.
func flowFixture(name string) string {
	return filepath.Join("..", "testdata", "flow", name)
}

// TestRenderAllShipsFlowDocuments proves that rendering a flow skill
// (issue #129) copies the flow documents into every target's rendered
// tree, byte-identical, for both layout conventions.
func TestRenderAllShipsFlowDocuments(t *testing.T) {
	cases := []struct {
		name    string
		fixture string
		flows   []string
	}{
		{
			name:    "flows directory layout",
			fixture: "browser-flows",
			flows:   []string{"flows/checkout.yaml", "flows/cleanup.yaml"},
		},
		{
			name:    "root flow file layout",
			fixture: "browser-root-flow",
			flows:   []string{"checkout.flow.yaml"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			bundle, err := skill.LoadBundle(flowFixture(tc.fixture))
			if err != nil {
				t.Fatalf("LoadBundle: %v", err)
			}

			out := filepath.Join(t.TempDir(), "rendered")
			results, errs := RenderAll(bundle, out, []Target{TargetOpenCode, TargetClaude})
			if len(errs) != 0 {
				t.Fatalf("RenderAll errors: %v", errs)
			}
			if len(results) != 2 {
				t.Fatalf("want 2 rendered targets, got %d", len(results))
			}
			for _, result := range results {
				for _, flow := range tc.flows {
					got, err := os.ReadFile(filepath.Join(result.Path, filepath.FromSlash(flow)))
					if err != nil {
						t.Fatalf("%s: flow document %q missing from rendered tree: %v", result.Target, flow, err)
					}
					want, err := os.ReadFile(filepath.Join(bundle.Root, filepath.FromSlash(flow)))
					if err != nil {
						t.Fatal(err)
					}
					if string(got) != string(want) {
						t.Errorf("%s: flow document %q content changed by render", result.Target, flow)
					}
				}
			}
		})
	}
}
