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
// token, no release, tag or tap publication, and signing only on explicit
// opt-in.
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
	for _, forbidden := range []string{"gh release", "git tag", "git push", "contents: write", "homebrew-tap"} {
		if strings.Contains(text, forbidden) {
			t.Errorf("workflow must not contain %q", forbidden)
		}
	}
	// Secrets and OIDC are confined to the opt-in signing job.
	var jobs struct {
		Jobs map[string]map[string]any `yaml:"jobs"`
	}
	if err := yaml.Unmarshal(raw, &jobs); err != nil {
		t.Fatal(err)
	}
	for name, body := range jobs.Jobs {
		encoded, err := yaml.Marshal(body)
		if err != nil {
			t.Fatal(err)
		}
		privileged := strings.Contains(string(encoded), "secrets.") || strings.Contains(string(encoded), "id-token") ||
			body["environment"] != nil
		if name != "sign" && privileged {
			t.Errorf("job %s must not use secrets, id-token or an environment", name)
		}
	}
	sign := jobs.Jobs["sign"]
	if sign["if"] != "github.event_name == 'workflow_dispatch' && inputs.sign" || sign["environment"] != "release" {
		t.Errorf("sign job must run only on dispatch with sign=true in the release environment: if=%v environment=%v", sign["if"], sign["environment"])
	}
	if permissions, _ := sign["permissions"].(map[string]any); len(permissions) != 2 ||
		permissions["contents"] != "read" || permissions["id-token"] != "write" {
		t.Errorf("sign job permissions = %v, want contents: read and id-token: write", sign["permissions"])
	}
	inputs, _ := w.On["workflow_dispatch"]["inputs"].(map[string]any)
	if signInput, _ := inputs["sign"].(map[string]any); signInput["default"] != false || signInput["type"] != "boolean" {
		t.Errorf("sign input must be a boolean defaulting to false: %v", inputs["sign"])
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
	for _, command := range []string{"smoke.py", "-hybrid-candidate-check -platforms all", "cyclonedx-json",
		"notarize-and-verify.sh", "cosign verify-blob", "--options runtime", "manifests.py"} {
		if !strings.Contains(text, command) {
			t.Errorf("missing release gate %q", command)
		}
	}
}
