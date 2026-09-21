// Command skills-runner-oracle freezes the Go skillsrunner.Run contract.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skillsrunner"
)

type caseFixture struct {
	Name      string                `json:"name"`
	Harnesses []string              `json:"harnesses"`
	DryRun    bool                  `json:"dry_run"`
	Timeout   time.Duration         `json:"timeout,omitempty"`
	Results   []skillsrunner.Result `json:"results"`
	RunError  string                `json:"run_error,omitempty"`
}

type fixture struct {
	Cases      []caseFixture  `json:"cases"`
	Defaults   []defaultsCase `json:"defaults"`
	Disk       []diskEntry    `json:"disk"`
	Provenance *provenance    `json:"provenance"`
}

// diskEntry is one file or symlink a real run left behind, recorded relative to
// the scenario root ($ROOT). For a symlink the target is the point: it is where
// the installed skill actually lives, and it is what a mode/`status` check
// follows. Files carry a content hash so a consumer can tell identical content
// at a different location from different content.
type diskEntry struct {
	Path   string          `json:"path"`
	Type   string          `json:"type"`
	Target string          `json:"target,omitempty"`
	SHA256 string          `json:"sha256,omitempty"`
	Marker *map[string]any `json:"marker,omitempty"`
}

// normalizedMarker removes the fields a real install fills with per-run state —
// the wall-clock `installed` stamp — and rewrites the temp root out of path
// values, so the remaining fields can be compared across runs and machines.
func normalizedMarker(marker map[string]any, root string) *map[string]any {
	delete(marker, "installed")
	for key, value := range marker {
		if text, ok := value.(string); ok {
			marker[key] = replaceRoot(text, root)
		}
	}
	return &marker
}

// defaultsCase freezes one directory-resolution scenario: the environment Go
// resolved under, and the three option paths config.Defaults() produced. Paths
// are recorded relative to the scenario root, which is replaced by the literal
// $ROOT so the fixture stays machine-independent.
type defaultsCase struct {
	Name        string `json:"name"`
	XDGDataHome string `json:"xdg_data_home"`
	LibraryDir  string `json:"library_dir"`
	RenderDir   string `json:"render_dir"`
	BaseDir     string `json:"base_dir"`
	Legacy      bool   `json:"legacy"`
	// CurrentCreated and LegacyCreated record which marker directories the
	// scenario created, so a consumer can rebuild the exact filesystem state.
	CurrentCreated bool `json:"current_created"`
	LegacyCreated  bool `json:"legacy_created"`
}

// provenance records where the fixture came from, so a reader can reproduce it.
//
// The recorded revision is the working-tree revision the fixture was captured
// from, which for an unmerged branch is that branch's commit: a squash merge
// destroys it, so the canonical check via scripts/run-go-oracle.sh is re-run on
// the merged main revision afterwards (see the note field).
type provenance struct {
	Revision  string `json:"go_revision"`
	Command   string `json:"generator_command"`
	Toolchain string `json:"go_toolchain"`
	Note      string `json:"note"`
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

	// 7. Dry-run failure names the target (planSkill path). This pins whether
	// the `target <t>: ` prefix belongs to the render layer or only to the
	// install path.
	{
		broken := filepath.Join(opts.LibraryDir, "broken")
		os.MkdirAll(broken, 0755)
		os.WriteFile(filepath.Join(broken, "SKILL.md"), []byte("---\nname: broken\ndescription: Broken\n---\n\n<!-- symskills:blok typo -->\n# Bad\nBody.\n"), 0644)

		r, err := skillsrunner.Run(context.Background(), []string{"claude"}, opts, true)
		cases = append(cases, caseFixture{
			Name: "dry_run_failure_names_target", Harnesses: []string{"claude"}, DryRun: true,
			Results: r, RunError: errStr(err),
		})

		os.RemoveAll(broken)
	}

	os.RemoveAll(home)

	disk, err := generateDisk()
	if err != nil {
		return fixture{}, err
	}

	return fixture{Cases: cases, Defaults: generateDefaults(), Disk: disk, Provenance: provenanceNow()}, nil
}

// generateDisk freezes the filesystem a real run leaves behind: the rendered
// tree, the harness roots under HOME, and the target each installed symlink
// actually points at. Directories are not recorded (they are implied by the
// file paths); the skills library is skipped because the caller created it.
func generateDisk() ([]diskEntry, error) {
	root, err := os.MkdirTemp("", "symbrain-runner-disk-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(root)

	opts := skillsrunner.DefaultOptions()
	opts.HomeDir = root
	opts.LibraryDir = filepath.Join(root, "library")
	opts.RenderDir = filepath.Join(root, "rendered")
	opts.BaseDir = filepath.Join(root, "base")

	demo := filepath.Join(opts.LibraryDir, "demo")
	if err := os.MkdirAll(demo, 0755); err != nil {
		return nil, err
	}
	if err := os.WriteFile(
		filepath.Join(demo, "SKILL.md"),
		[]byte("---\nname: demo\ndescription: test\n---\n\n# Demo\nBody.\n"),
		0644,
	); err != nil {
		return nil, err
	}

	if _, err := skillsrunner.Run(
		context.Background(),
		[]string{"claude", "codex", "hermes", "opencode"},
		opts,
		false,
	); err != nil {
		return nil, err
	}

	return captureDisk(root, opts.LibraryDir)
}

// captureDisk records every file and symlink under root, relative to it, sorted
// by path. skipDir (the skills library) is not descended into.
func captureDisk(root, skipDir string) ([]diskEntry, error) {
	entries := []diskEntry{}
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if path == skipDir {
			return fs.SkipDir
		}
		if d.IsDir() {
			return nil
		}
		rel, relErr := filepath.Rel(root, path)
		if relErr != nil {
			return relErr
		}
		entry := diskEntry{Path: replaceRoot(rel, root)}
		info, infoErr := os.Lstat(path)
		if infoErr != nil {
			return infoErr
		}
		if info.Mode()&os.ModeSymlink != 0 {
			target, linkErr := os.Readlink(path)
			if linkErr != nil {
				return linkErr
			}
			entry.Type = "symlink"
			entry.Target = replaceRoot(target, root)
		} else {
			entry.Type = "file"
			data, readErr := os.ReadFile(path)
			if readErr != nil {
				return readErr
			}
			if filepath.Base(path) == ".symskills.json" {
				// The marker embeds `installed: <RFC3339 now>`, so its bytes are
				// not reproducible by construction; hashing them would make the
				// fixture drift on every run. Record the fields that are
				// deterministic and keep the volatile timestamp out.
				var marker map[string]any
				if json.Unmarshal(data, &marker) != nil {
					return fmt.Errorf("marker %s is not JSON", path)
				}
				entry.Marker = normalizedMarker(marker, root)
			} else {
				sum := sha256.Sum256(data)
				entry.SHA256 = hex.EncodeToString(sum[:])
			}
		}
		entries = append(entries, entry)
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Path < entries[j].Path })
	return entries, nil
}

// generateDefaults freezes the directory resolution of config.Defaults() as
// reached through skillsrunner.DefaultOptions, one scenario per branch of
// internal/paths.resolve: current $XDG_DATA_HOME/symbrain/skills, the legacy
// $XDG_DATA_HOME/symskills fallback, neither existing, a relative
// XDG_DATA_HOME (ignored per the XDG spec) and XDG_DATA_HOME unset.
func generateDefaults() []defaultsCase {
	savedHome, hadHome := os.LookupEnv("HOME")
	savedXDG, hadXDG := os.LookupEnv("XDG_DATA_HOME")
	defer func() {
		if hadHome {
			os.Setenv("HOME", savedHome)
		} else {
			os.Unsetenv("HOME")
		}
		if hadXDG {
			os.Setenv("XDG_DATA_HOME", savedXDG)
		} else {
			os.Unsetenv("XDG_DATA_HOME")
		}
	}()

	var out []defaultsCase
	root, err := os.MkdirTemp("", "symbrain-runner-defaults-")
	if err != nil {
		return out
	}
	defer os.RemoveAll(root)

	scenarios := []struct {
		name      string
		xdg       string // subpath under root, a literal relative path, or "" for unset
		absolute  bool
		mkCurrent bool
		mkLegacy  bool
	}{
		{name: "xdg_absolute_current_exists", xdg: "data", absolute: true, mkCurrent: true},
		{name: "xdg_absolute_legacy_only", xdg: "data", absolute: true, mkLegacy: true},
		{name: "xdg_absolute_neither_exists", xdg: "data", absolute: true},
		{name: "xdg_relative_is_ignored", xdg: "rel/data", mkCurrent: true},
		{name: "xdg_unset_uses_home", mkCurrent: true},
	}

	for _, scenario := range scenarios {
		// Each scenario gets its own root, otherwise the directory the previous
		// scenario created would make the current branch win and hide the
		// legacy fallback entirely.
		scenarioRoot := filepath.Join(root, scenario.name)
		homeDir := filepath.Join(scenarioRoot, "home")
		if err := os.MkdirAll(homeDir, 0755); err != nil {
			continue
		}
		os.Setenv("HOME", homeDir)

		dataBase := filepath.Join(homeDir, ".local", "share")
		switch {
		case scenario.xdg == "":
			os.Unsetenv("XDG_DATA_HOME")
		case scenario.absolute:
			absolute := filepath.Join(scenarioRoot, scenario.xdg)
			os.Setenv("XDG_DATA_HOME", absolute)
			dataBase = absolute
		default:
			// A relative value must be ignored, so the environment keeps a
			// relative path while resolution falls back to HOME.
			os.Setenv("XDG_DATA_HOME", scenario.xdg)
		}

		if scenario.mkCurrent {
			os.MkdirAll(filepath.Join(dataBase, "symbrain", "skills"), 0755)
		}
		if scenario.mkLegacy {
			os.MkdirAll(filepath.Join(dataBase, "symskills"), 0755)
		}

		opts := skillsrunner.DefaultOptions()
		out = append(out, defaultsCase{
			Name:        scenario.name,
			XDGDataHome: replaceRoot(os.Getenv("XDG_DATA_HOME"), root),
			LibraryDir:  replaceRoot(opts.LibraryDir, root),
			RenderDir:   replaceRoot(opts.RenderDir, root),
			BaseDir:     replaceRoot(opts.BaseDir, root),
			Legacy:      filepath.Base(filepath.Dir(opts.LibraryDir)) == "symskills",

			CurrentCreated: scenario.mkCurrent,
			LegacyCreated:  scenario.mkLegacy,
		})
	}

	return out
}

// replaceRoot makes a recorded path machine-independent.
func replaceRoot(path, root string) string {
	return strings.ReplaceAll(path, root, "$ROOT")
}

// provenanceNow records the revision, command and toolchain a fixture was
// captured from.
func provenanceNow() *provenance {
	return &provenance{
		Revision:  gitOutput("rev-parse", "HEAD"),
		Command:   "go run ./scripts/skills-runner-oracle",
		Toolchain: goToolchain(),
		Note: "Captured from this working tree. A squash merge destroys the " +
			"recorded revision, so re-run scripts/run-go-oracle.sh with the " +
			"merged main revision and regenerate before trusting the pin.",
	}
}

func gitOutput(args ...string) string {
	out, err := exec.Command("git", args...).Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

// goToolchain reads the toolchain requirement from go.mod, the same way
// scripts/run-go-oracle.sh does.
func goToolchain() string {
	data, err := os.ReadFile("go.mod")
	if err != nil {
		return "unknown"
	}
	for _, line := range strings.Split(string(data), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 2 && fields[0] == "go" {
			return "go" + fields[1]
		}
	}
	return "unknown"
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
	if len(a.Cases) != len(b.Cases) || len(a.Defaults) != len(b.Defaults) || len(a.Disk) != len(b.Disk) {
		return false
	}
	for i := range a.Disk {
		// Marker is a pointer to a map, so a struct `!=` would compare
		// addresses and report drift on every run. Compare the payload.
		if a.Disk[i].Path != b.Disk[i].Path ||
			a.Disk[i].Type != b.Disk[i].Type ||
			a.Disk[i].Target != b.Disk[i].Target ||
			a.Disk[i].SHA256 != b.Disk[i].SHA256 ||
			!reflect.DeepEqual(a.Disk[i].Marker, b.Disk[i].Marker) {
			return false
		}
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
	// Provenance is deliberately not compared: it records the revision a
	// fixture was captured from, which changes with every commit.
	for i := range a.Defaults {
		ad, bd := a.Defaults[i], b.Defaults[i]
		if ad.Name != bd.Name || ad.LibraryDir != bd.LibraryDir ||
			ad.RenderDir != bd.RenderDir || ad.BaseDir != bd.BaseDir || ad.Legacy != bd.Legacy {
			return false
		}
	}
	return true
}
