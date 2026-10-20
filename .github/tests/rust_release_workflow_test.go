package workflows

import (
	"regexp"
	"slices"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// The Rust release candidate pipeline must stay separate from the published
// v* release until the cutover (#782): no tag or push trigger, read-only
// token, no secrets and no release, tag or tap publication.
func TestRustReleaseWorkflowBuildsAllTargetsWithoutPublishing(t *testing.T) {
	raw := repositoryFile(t, ".github/workflows/rust-release.yml")
	var w workflow
	if err := yaml.Unmarshal(raw, &w); err != nil {
		t.Fatal(err)
	}
	events := make([]string, 0, len(w.On))
	for event := range w.On {
		events = append(events, event)
	}
	slices.Sort(events)
	if !slices.Equal(events, []string{"pull_request", "workflow_dispatch"}) {
		t.Errorf("triggers = %v, want only pull_request and workflow_dispatch", events)
	}
	if len(w.Permissions) != 1 || w.Permissions["contents"] != "read" {
		t.Errorf("permissions = %v, want contents: read only", w.Permissions)
	}
	text := string(raw)
	for _, forbidden := range []string{"secrets.", "gh release", "git tag", "git push", "id-token", "contents: write", "homebrew-tap"} {
		if strings.Contains(text, forbidden) {
			t.Errorf("workflow must not contain %q", forbidden)
		}
	}
	for _, line := range regexp.MustCompile(`(?m)^\s*-?\s*uses:\s*(\S+)`).FindAllStringSubmatch(text, -1) {
		if !regexp.MustCompile(`@[0-9a-f]{40}$`).MatchString(line[1]) {
			t.Errorf("unpinned action %s", line[1])
		}
	}

	var parsed struct {
		Jobs map[string]struct {
			Strategy struct {
				Matrix struct {
					Include []map[string]string `yaml:"include"`
				} `yaml:"matrix"`
			} `yaml:"strategy"`
		} `yaml:"jobs"`
	}
	if err := yaml.Unmarshal(raw, &parsed); err != nil {
		t.Fatal(err)
	}
	var platforms []string
	for _, entry := range parsed.Jobs["build"].Strategy.Matrix.Include {
		platforms = append(platforms, entry["os"]+"/"+entry["arch"])
	}
	slices.Sort(platforms)
	want := []string{"darwin/amd64", "darwin/arm64", "linux/amd64", "linux/arm64", "windows/amd64", "windows/arm64"}
	if !slices.Equal(platforms, want) {
		t.Errorf("build platforms = %v, want %v", platforms, want)
	}
	for _, command := range []string{"smoke.py", "-hybrid-candidate-check -platforms all", "cyclonedx-json"} {
		if !strings.Contains(text, command) {
			t.Errorf("missing release gate %q", command)
		}
	}
}
