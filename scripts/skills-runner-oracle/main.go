// Command skills-runner-oracle freezes the Go skillsrunner.Run contract.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skillsrunner"
)

type caseFixture struct {
	Name        string                      `json:"name"`
	Harnesses   []string                    `json:"harnesses"`
	DryRun      bool                        `json:"dry_run"`
	Timeout     time.Duration               `json:"timeout,omitempty"`
	Results     []skillsrunner.Result      `json:"results"`
	RunError    string                      `json:"run_error,omitempty"`
}

type fixture struct {
	Cases []caseFixture `json:"cases"`
}

func generate() (fixture, error) {
	// Use temp HOME/XDG roots so the fixture is isolated from operator state
	home := os.TempDir() + "/symbrain-runner-oracle-" + fmt.Sprintf("%d", os.Getpid())
	os.RemoveAll(home)
	os.MkdirAll(filepath.Join(home, ".local", "share", "symskills", "library"), 0755)
	os.MkdirAll(filepath.Join(home, ".local", "share", "symskills", "rendered"), 0755)
	os.MkdirAll(filepath.Join(home, ".local", "share", "symskills", "base"), 0755)

	opts := skillsrunner.DefaultOptions()
	opts.HomeDir = home
	opts.LibraryDir = filepath.Join(home, ".local", "share", "symskills", "library")
	opts.RenderDir = filepath.Join(home, ".local", "share", "symskills", "rendered")
	opts.BaseDir = filepath.Join(home, ".local", "share", "symskills", "base")

	var cases []caseFixture

	// 1. Empty library (TestRun_EmptyLibraryIsNotAnError)
	{
		r, err := skillsrunner.Run(context.Background(), []string{"claude"}, opts, false)
		cases = append(cases, caseFixture{
			Name: "empty_library_not_error", Harnesses: []string{"claude"}, DryRun: false,
			Results: r, RunError: errStr(err),
		})
	}

	// 2. Unsupported harnesses (TestRun_UnsupportedHarness)
	{
		r, err := skillsrunner.Run(context.Background(), []string{"cursor", "claude-desktop"}, opts, false)
		cases = append(cases, caseFixture{
			Name: "unsupported_harness_skipped", Harnesses: []string{"cursor", "claude-desktop"}, DryRun: false,
			Results: r, RunError: errStr(err),
		})
	}

	// 3. Broken skill visible (TestRun_FailureIsVisibleAndReported)
	{
		library := opts.LibraryDir
		demo := filepath.Join(library, "demo")
		os.MkdirAll(demo, 0755)
		os.WriteFile(filepath.Join(demo, "SKILL.md"), []byte("---\nname: demo\ndescription: A demo\n---\n\n# Demo\nBody.\n"), 0644)
		broken := filepath.Join(library, "broken")
		os.MkdirAll(broken, 0755)
		os.WriteFile(filepath.Join(broken, "SKILL.md"), []byte("---\nname: broken\ndescription: Broken\n---\n\n<!-- symskills:blok typo -->\n# Bad\nBody.\n"), 0644)

		r, err := skillsrunner.Run(context.Background(), []string{"claude"}, opts, false)
		cases = append(cases, caseFixture{
			Name: "failure_visible_and_reported", Harnesses: []string{"claude"}, DryRun: false,
			Results: r, RunError: errStr(err),
		})

		os.RemoveAll(filepath.Join(library, "demo"))
		os.RemoveAll(filepath.Join(library, "broken"))
	}

	// 4. No legacy binary, targets processed (TestRun_NoLegacyBinaryProcessesTargets)
	{
		demo := filepath.Join(opts.LibraryDir, "demo")
		os.MkdirAll(demo, 0755)
		os.WriteFile(filepath.Join(demo, "SKILL.md"), []byte("---\nname: demo\ndescription: A demo skill for tests.\n---\n\n# Demo\nBody.\n"), 0644)
		noclient := filepath.Join(opts.LibraryDir, "noclient")
		os.MkdirAll(noclient, 0755)
		os.WriteFile(filepath.Join(noclient, "symskills.toml"), []byte("[targets.claude]\nenabled = false\n"), 0644)

		r, err := skillsrunner.Run(context.Background(), []string{"claude", "codex", "hermes", "opencode"}, opts, false)
		cases = append(cases, caseFixture{
			Name: "no_legacy_binary_processes_targets", Harnesses: []string{"claude", "codex", "hermes", "opencode"}, DryRun: false,
			Results: r, RunError: errStr(err),
		})

		os.RemoveAll(demo)
		os.RemoveAll(noclient)
	}

	// 5. Timeout surfaces error (TestRunTimeoutSurfacesAnError)
	{
		demo := filepath.Join(opts.LibraryDir, "demo")
		os.MkdirAll(demo, 0755)
		os.WriteFile(filepath.Join(demo, "SKILL.md"), []byte("---\nname: demo\ndescription: A demo\n---\n\n# Demo\nBody.\n"), 0644)

		timeoutOpts := opts
		timeoutOpts.Timeout = 1 // 1 nanosecond

		r, err := skillsrunner.Run(context.Background(), []string{"claude"}, timeoutOpts, false)
		cases = append(cases, caseFixture{
			Name: "timeout_surfaces_error", Harnesses: []string{"claude"}, DryRun: false, Timeout: 1,
			Results: r, RunError: errStr(err),
		})

		os.RemoveAll(demo)
	}

	// 6. Binary presence changes nothing (baseline comparison)
	{
		demo := filepath.Join(opts.LibraryDir, "demo")
		os.MkdirAll(demo, 0755)
		os.WriteFile(filepath.Join(demo, "SKILL.md"), []byte("---\nname: demo\ndescription: A demo\n---\n\n# Demo\nBody.\n"), 0644)

		r, err := skillsrunner.Run(context.Background(), []string{"claude", "codex"}, opts, true)
		cases = append(cases, caseFixture{
			Name: "binary_presence_changes_nothing", Harnesses: []string{"claude", "codex"}, DryRun: true,
			Results: r, RunError: errStr(err),
		})

		os.RemoveAll(demo)
	}

	os.RemoveAll(home)

	return fixture{Cases: cases}, nil
}

func errStr(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func main() {
	check := flag.Bool("check", false, "check fixture against Go behavior (fail on drift)")
	flag.Parse()

	fixturePath := "rust/symbrain-skills/tests/fixtures/runner_oracle.json"

	if *check {
		data, err := os.ReadFile(fixturePath)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read fixture: %v\n", err)
			os.Exit(1)
		}
		var stored fixture
		if err := json.Unmarshal(data, &stored); err != nil {
			fmt.Fprintf(os.Stderr, "parse fixture: %v\n", err)
			os.Exit(1)
		}
		current, err := generate()
		if err != nil {
			fmt.Fprintf(os.Stderr, "generate: %v\n", err)
			os.Exit(1)
		}
		if !equalFixture(stored, current) {
			fmt.Fprintf(os.Stderr, "fixture drift detected (Go behavior changed)\n")
			os.Exit(1)
		}
		fmt.Println("fixture matches Go (check passed)")
		return
	}

	current, err := generate()
	if err != nil {
		fmt.Fprintf(os.Stderr, "generate: %v\n", err)
		os.Exit(1)
	}
	data, err := json.MarshalIndent(current, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "marshal: %v\n", err)
		os.Exit(1)
	}
	os.MkdirAll(filepath.Dir(fixturePath), 0755)
	if err := os.WriteFile(fixturePath, data, 0644); err != nil {
		fmt.Fprintf(os.Stderr, "write: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("fixture written to %s (%d cases)\n", fixturePath, len(current.Cases))
}

func equalFixture(a, b fixture) bool {
	if len(a.Cases) != len(b.Cases) {
		return false
	}
	for i := range a.Cases {
		ac, bc := a.Cases[i], b.Cases[i]
		if ac.Name != bc.Name || ac.DryRun != bc.DryRun || len(ac.Results) != len(bc.Results) || ac.RunError != bc.RunError {
			return false
		}
		for j := range ac.Results {
			ar, br := ac.Results[j], bc.Results[j]
			if ar.Target != br.Target || ar.Status != br.Status || ar.Message != br.Message {
				return false
			}
		}
	}
	return true
}
