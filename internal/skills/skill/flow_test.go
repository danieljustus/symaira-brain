package skill

import (
	"os"
	"path/filepath"
	"testing"

	"gopkg.in/yaml.v3"
)

// flowFixture returns the testdata path of a flow-skill fixture. The
// fixtures live in the top-level testdata directory so every package
// (skill, render, cmd/symskills) shares them.
func flowFixture(name string) string {
	return filepath.Join("..", "testdata", "flow", name)
}

// TestFlowSkillLoadsFlowDocumentsAsResources proves that flow skills
// (issue #129) load and validate at the skill level with their flow
// documents inventoried as ordinary resources — for both layout
// conventions: a flows/ subdirectory of *.yaml documents and a single
// *.flow.yaml file in the skill root.
func TestFlowSkillLoadsFlowDocumentsAsResources(t *testing.T) {
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
			bundle, err := LoadBundle(flowFixture(tc.fixture))
			if err != nil {
				t.Fatalf("LoadBundle: %v", err)
			}

			byPath := map[string]Resource{}
			for _, res := range bundle.Resources {
				byPath[res.Path] = res
			}
			for _, flow := range tc.flows {
				res, ok := byPath[flow]
				if !ok {
					t.Fatalf("flow document %q missing from bundle resources: %#v", flow, byPath)
				}
				if res.Executable {
					t.Errorf("flow document %q must not be executable (YAML data file, not a script): %#v", flow, res)
				}
				if res.Size == 0 {
					t.Errorf("flow document %q has zero size: %#v", flow, res)
				}
				// Flow documents must be well-formed YAML documents; the
				// schema itself is validated by the executing tool
				// (symbrowse), not by symskills.
				raw, err := os.ReadFile(filepath.Join(bundle.Root, filepath.FromSlash(flow)))
				if err != nil {
					t.Fatalf("read flow document %q: %v", flow, err)
				}
				var doc map[string]any
				if err := yaml.Unmarshal(raw, &doc); err != nil {
					t.Errorf("flow document %q is not well-formed YAML: %v", flow, err)
				}
				if doc["name"] == "" {
					t.Errorf("flow document %q has no name field", flow)
				}
			}

			// Flow skills must validate clean at the skill level: flow
			// documents are ordinary data resources, so no error-severity
			// issue may appear.
			for _, issue := range Validate(bundle) {
				if issue.Severity == "error" {
					t.Errorf("unexpected error-severity issue for flow skill: %#v", issue)
				}
			}
		})
	}
}
