// Package workflows checks that Rust migration CI preserves executable gates.
package workflows

import (
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/BurntSushi/toml"
	"gopkg.in/yaml.v3"
)

type workflow struct {
	On          map[string]map[string]any `yaml:"on"`
	Permissions map[string]string         `yaml:"permissions"`
	Jobs        map[string]job            `yaml:"jobs"`
}

type job struct {
	RunsOn         string           `yaml:"runs-on"`
	TimeoutMinutes int              `yaml:"timeout-minutes"`
	Steps          []map[string]any `yaml:"steps"`
	Other          map[string]any   `yaml:",inline"`
}

func repositoryFile(t *testing.T, path string) []byte {
	t.Helper()
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("cannot locate workflow tests")
	}
	data, err := os.ReadFile(filepath.Join(filepath.Dir(source), "../..", path))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func rustWorkflow(t *testing.T) workflow {
	t.Helper()
	var parsed workflow
	if err := yaml.Unmarshal(repositoryFile(t, ".github/workflows/rust.yml"), &parsed); err != nil {
		t.Fatal(err)
	}
	return parsed
}

func TestRustWorkflowPreservesPullRequests(t *testing.T) {
	w := rustWorkflow(t)
	for _, event := range []string{"push", "pull_request"} {
		t.Run(event, func(t *testing.T) {
			config, ok := w.On[event]
			if !ok {
				t.Fatalf("missing %s trigger", event)
			}
			branches, ok := config["branches"].([]any)
			if !ok || len(branches) != 1 || branches[0] != "main" {
				t.Fatalf("expected main branch trigger, got %v", config)
			}
			for _, filter := range []string{"paths", "paths-ignore", "branches-ignore", "types"} {
				if _, present := config[filter]; present {
					t.Errorf("%s must not exclude Rust-relevant changes through %s", event, filter)
				}
			}
		})
	}
	if len(w.Permissions) != 1 || w.Permissions["contents"] != "read" {
		t.Errorf("Rust gate requires contents: read only, got %v", w.Permissions)
	}
}

func TestRustWorkflowJobsFailClosed(t *testing.T) {
	w := rustWorkflow(t)
	if len(w.Jobs) != 3 {
		t.Errorf("expected three independent Rust gates, got %d", len(w.Jobs))
	}
	for _, name := range []string{"guard-parity", "cargo-audit", "cargo-deny"} {
		t.Run(name, func(t *testing.T) {
			j, ok := w.Jobs[name]
			if !ok {
				t.Fatalf("missing job %s", name)
			}
			if j.RunsOn != "ubuntu-latest" || j.TimeoutMinutes <= 0 || j.TimeoutMinutes > 20 {
				t.Errorf("expected bounded Ubuntu gate, got runner %q timeout %d", j.RunsOn, j.TimeoutMinutes)
			}
			for _, key := range []string{"if", "needs", "continue-on-error", "permissions"} {
				if _, present := j.Other[key]; present {
					t.Errorf("job %s must not override %s", name, key)
				}
			}
			checkout := false
			for _, step := range j.Steps {
				for _, key := range []string{"if", "continue-on-error"} {
					if _, present := step[key]; present {
						t.Errorf("step %v must not override %s", step["name"], key)
					}
				}
				if uses, ok := step["uses"].(string); ok {
					if !regexp.MustCompile(`@[0-9a-f]{40}$`).MatchString(uses) {
						t.Errorf("unpinned action %s", uses)
					}
					if strings.HasPrefix(uses, "actions/checkout@") {
						checkout = true
						if uses != "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1" {
							t.Errorf("checkout pin differs from repository convention: %s", uses)
						}
					}
				}
			}
			if !checkout {
				t.Error("missing checkout")
			}
			requireCommand(t, jobCommands(j), "rustup show", "active-toolchain")
		})
	}
}

func jobCommands(j job) string {
	var commands []string
	for _, step := range j.Steps {
		if run, ok := step["run"].(string); ok {
			commands = append(commands, run)
		}
	}
	return strings.Join(commands, "\n")
}

func requireCommand(t *testing.T, script, command string, required ...string) {
	t.Helper()
	script = strings.ReplaceAll(script, "\\\n", " ")
	for _, line := range strings.Split(script, "\n") {
		if !strings.Contains(line, command) {
			continue
		}
		fields := strings.Fields(line)
		if slices.ContainsFunc(required, func(flag string) bool { return !slices.Contains(fields, flag) }) {
			continue
		}
		return
	}
	t.Errorf("missing command %q with arguments %v", command, required)
}

func TestRustWorkflowPinsToolsAndOracleHistory(t *testing.T) {
	w := rustWorkflow(t)
	parity := w.Jobs["guard-parity"]
	history, goSetup := false, false
	for _, step := range parity.Steps {
		with, _ := step["with"].(map[string]any)
		uses, _ := step["uses"].(string)
		if strings.HasPrefix(uses, "actions/checkout@") {
			history = with["fetch-depth"] == 0
		}
		if uses == "actions/setup-go@b7ad1dad31e06c5925ef5d2fc7ad053ef454303e" {
			goSetup = with["go-version-file"] == "go.mod"
		}
	}
	if !history || !goSetup {
		t.Errorf("oracle requires full pinned history and repository Go version: history=%v Go=%v", history, goSetup)
	}
	requireCommand(t, jobCommands(parity), "go test", "./.github/tests", "-count=1")
	requireCommand(t, jobCommands(parity), "make", "rust-guard-check")
	for _, tool := range []struct{ name, version, target string }{
		{"cargo-audit", "0.22.2", "rust-audit"},
		{"cargo-deny", "0.20.2", "rust-deny"},
	} {
		t.Run(tool.name, func(t *testing.T) {
			commands := jobCommands(w.Jobs[tool.name])
			requireCommand(t, commands, "cargo install", tool.name, "--version", tool.version, "--locked")
			requireCommand(t, commands, "make", tool.target)
		})
	}
}

func makeRecipe(t *testing.T, name string) string {
	t.Helper()
	lines := strings.Split(string(repositoryFile(t, "Makefile")), "\n")
	var recipe []string
	inTarget := false
	for _, line := range lines {
		if strings.HasPrefix(line, name+":") {
			inTarget = true
			continue
		}
		if inTarget && strings.HasPrefix(line, "\t") {
			recipe = append(recipe, line)
		} else if inTarget && strings.TrimSpace(line) != "" {
			break
		}
	}
	if len(recipe) == 0 {
		t.Fatalf("missing executable recipe %s", name)
	}
	return strings.Join(recipe, "\n")
}

func TestRustMakeGatesUseLockedReadOnlyChecks(t *testing.T) {
	parity := makeRecipe(t, "rust-guard-check")
	requireCommand(t, string(repositoryFile(t, "Makefile")), "GO_ORACLE_TOOLCHAIN :=", "go.mod)")
	requireCommand(t, parity, "GOTOOLCHAIN=$(GO_ORACLE_TOOLCHAIN) go run", "./guard/scripts/guard-oracle", "-check")
	requireCommand(t, parity, "GOTOOLCHAIN=$(GO_ORACLE_TOOLCHAIN) go test", "./guard/internal/capability", "-count=1")
	if !strings.Contains(parity, "TestCapabilityOracle") || !strings.Contains(parity, "Fixture") || !strings.Contains(parity, "RejectsDrift") {
		t.Error("missing pinned capability fixture and drift-rejection tests")
	}
	requireCommand(t, parity, "cargo fmt", "--all", "--check")
	for _, command := range []string{"check", "clippy", "test"} {
		t.Run(command, func(t *testing.T) {
			requireCommand(t, parity, "cargo "+command, "--workspace", "--all-targets", "--all-features", "--locked")
		})
	}
	requireCommand(t, parity, "cargo clippy", "--", "-D", "warnings")
	requireCommand(t, parity, "cargo test", "--doc", "--workspace", "--all-features", "--locked")
	for _, unsafe := range []string{"SYMBRAIN_CAPABILITY_UPDATE=1", "cargo update", "|| true", "set +e"} {
		if strings.Contains(parity, unsafe) {
			t.Errorf("check recipe must not mutate fixtures or suppress failures: %s", unsafe)
		}
	}
	requireCommand(t, makeRecipe(t, "rust-audit"), "cargo audit", "--file", "Cargo.lock", "--deny", "warnings")
	requireCommand(t, makeRecipe(t, "rust-deny"), "cargo deny", "--locked", "--all-features", "check")
}

func TestRustMakePreservesDefaultTarget(t *testing.T) {
	// Ignore special targets and assignments when locating Make's default goal.
	first := regexp.MustCompile(`(?m)^([A-Za-z_][A-Za-z0-9_.-]*):([^=]|$)`).
		FindSubmatch(repositoryFile(t, "Makefile"))
	if len(first) == 0 || string(first[1]) != "coverage" {
		t.Fatalf("Rust gates must preserve the existing coverage default target, got %q", first)
	}
}

func TestRustDependencyPolicyRejectsUnknownSources(t *testing.T) {
	var policy struct {
		Advisories struct{ Ignore []string }
		Licenses   struct{ Allow []string }
		Sources    struct {
			UnknownRegistry string `toml:"unknown-registry"`
			UnknownGit      string `toml:"unknown-git"`
		}
	}
	if err := toml.Unmarshal(repositoryFile(t, "deny.toml"), &policy); err != nil {
		t.Fatal(err)
	}
	if len(policy.Advisories.Ignore) != 0 {
		t.Errorf("dependency gate must not silently ignore advisories: %v", policy.Advisories.Ignore)
	}
	if len(policy.Licenses.Allow) == 0 {
		t.Error("dependency gate requires an explicit license allowlist")
	}
	if policy.Sources.UnknownRegistry != "deny" || policy.Sources.UnknownGit != "deny" {
		t.Error("dependency gate must reject unknown registries and Git sources")
	}
}
