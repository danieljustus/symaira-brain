// Command profile-remove-oracle freezes the Go profile removal contract.
//
// Cases execute the production Go CLI in isolated HOME, XDG, and project
// roots. The fixture records source hashes so regeneration cannot silently
// relabel a changed oracle.
package main

import (
	"bytes"
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
	"runtime"
	"sort"
	"strings"
)

type fileState struct {
	Path  string `json:"path"`
	Type  string `json:"type"`
	Mode  uint32 `json:"mode"`
	Bytes []byte `json:"bytes,omitempty"`
}

type result struct {
	ID     string      `json:"id"`
	Args   []string    `json:"args"`
	Stdin  string      `json:"stdin,omitempty"`
	Exit   int         `json:"exit"`
	Stdout string      `json:"stdout"`
	Stderr string      `json:"stderr"`
	Files  []fileState `json:"files"`
}

type oracle struct {
	SchemaVersion   int               `json:"schema_version"`
	GoRevision      string            `json:"go_revision"`
	GeneratorSHA256 string            `json:"generator_sha256"`
	GoSources       map[string]string `json:"go_sources"`
	Cases           []result          `json:"cases"`
}

type caseDef struct {
	ID        string
	Args      []string
	Stdin     []byte
	Setup     func(root, home, config, project string) error
	PosixOnly bool
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-cli/tests/fixtures/profile_remove_oracle_"+runtime.GOOS+".json", "fixture path")
	flag.Parse()
	root := repoRoot()
	generated := generate(root)
	data, err := json.MarshalIndent(generated, "", "  ")
	if err != nil {
		fatalf("marshal oracle: %v", err)
	}
	data = append(data, '\n')
	if *check {
		existing, _ := os.ReadFile(*output)
		if !fixtureMatches(existing, generated) {
			fatalf("%s is out of date; run go run ./scripts/profile-remove-oracle", *output)
		}
		fmt.Printf("PASS: profile remove oracle deterministic check passed (%d cases)\n", len(generated.Cases))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o700); err != nil {
		fatalf("mkdir %s: %v", filepath.Dir(*output), err)
	}
	if err := os.WriteFile(*output, data, 0o600); err != nil {
		fatalf("write %s: %v", *output, err)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(generated.Cases))
}

// fixtureMatches ignores only the historical Git revision. Source hashes,
// generator identity, platform-selected cases, and exact behavior remain strict.
func fixtureMatches(existing []byte, generated oracle) bool {
	var expected oracle
	if err := json.Unmarshal(existing, &expected); err != nil {
		return false
	}
	return expected.SchemaVersion == generated.SchemaVersion &&
		expected.GeneratorSHA256 == generated.GeneratorSHA256 &&
		reflect.DeepEqual(expected.GoSources, generated.GoSources) &&
		reflect.DeepEqual(expected.Cases, generated.Cases)
}

func generate(root string) oracle {
	binaryFile, err := os.CreateTemp("", "symbrain-go-profile-remove-oracle-")
	if err != nil {
		fatalf("create Go binary: %v", err)
	}
	binary := binaryFile.Name()
	if err := binaryFile.Close(); err != nil {
		fatalf("close Go binary: %v", err)
	}
	defer os.Remove(binary)
	build := exec.Command("go", "build", "-o", binary, "./cmd/symbrain")
	build.Dir = root
	build.Env = append(os.Environ(), "CGO_ENABLED=0")
	if output, err := build.CombinedOutput(); err != nil {
		fatalf("build Go oracle: %v\n%s", err, output)
	}
	generated := oracle{
		SchemaVersion:   1,
		GoRevision:      gitRevision(root),
		GeneratorSHA256: fileSHA256(filepath.Join(root, "scripts", "profile-remove-oracle", "main.go")),
		GoSources:       sourceHashes(root),
	}
	for _, definition := range definitions() {
		if definition.PosixOnly && runtime.GOOS == "windows" {
			continue
		}
		generated.Cases = append(generated.Cases, runCase(binary, definition))
	}
	return generated
}

func definitions() []caseDef {
	return []caseDef{
		{ID: "missing", Args: []string{"profile", "remove", "missing", "--force"}},
		{ID: "force_existing", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupExisting},
		{ID: "prompt_yes", Args: []string{"profile", "remove", "existing"}, Stdin: []byte("yes\n"), Setup: setupExisting},
		{ID: "prompt_no", Args: []string{"profile", "remove", "existing"}, Stdin: []byte("n\n"), Setup: setupExisting},
		{ID: "prompt_eof", Args: []string{"profile", "remove", "existing"}, Setup: setupExisting},
		{ID: "prompt_oversized", Args: []string{"profile", "remove", "existing"}, Stdin: bytes.Repeat([]byte("x"), 1025), Setup: setupExisting},
		{ID: "bound_global_refuses", Args: []string{"profile", "remove", "bound"}, Setup: setupBoundGlobal},
		{ID: "bound_project_refuses", Args: []string{"profile", "remove", "existing", "--project", "PROJECT"}, Setup: setupBoundProject},
		{ID: "bound_project_relative_refuses", Args: []string{"profile", "remove", "existing", "--project", "."}, Setup: setupBoundProject},
		{ID: "bound_project_symlink_refuses", Args: []string{"profile", "remove", "existing", "--project", "project-link"}, Setup: setupBoundProjectSymlink, PosixOnly: true},
		{ID: "bound_force", Args: []string{"profile", "remove", "bound", "--force"}, Setup: setupBoundGlobal},
		{ID: "malformed_profile", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupMalformedProfile},
		{ID: "malformed_harness_config", Args: []string{"profile", "remove", "bound", "--force"}, Setup: setupMalformedHarness},
		{ID: "malformed_harness_refuses", Args: []string{"profile", "remove", "bound"}, Setup: setupMalformedHarness},
		{ID: "symlink", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupSymlink, PosixOnly: true},
		{ID: "symlink_profiles_root", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupSymlinkProfilesRoot, PosixOnly: true},
		{ID: "special_file", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupFIFO, PosixOnly: true},
		{ID: "empty_directory", Args: []string{"profile", "remove", "existing", "--force"}, Setup: setupDirectory},
		{ID: "force_before_name", Args: []string{"profile", "remove", "--force", "existing"}, Setup: setupExisting},
		{ID: "terminator", Args: []string{"profile", "remove", "existing", "--", "--force"}, Setup: setupExisting},
	}
}

func runCase(binary string, definition caseDef) result {
	root, err := os.MkdirTemp("", "symbrain-profile-remove-oracle-")
	if err != nil {
		fatalf("create case root: %v", err)
	}
	defer os.RemoveAll(root)
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		fatalf("canonicalize case root: %v", err)
	}
	home, config, project := filepath.Join(root, "home"), filepath.Join(root, "config"), filepath.Join(root, "project")
	for _, path := range []string{home, config, project} {
		if err := os.MkdirAll(path, 0o700); err != nil {
			fatalf("create case directory: %v", err)
		}
	}
	if definition.Setup != nil {
		if err := definition.Setup(root, home, config, project); err != nil {
			fatalf("setup %s: %v", definition.ID, err)
		}
	}
	args := append([]string(nil), definition.Args...)
	for i, arg := range args {
		if arg == "PROJECT" {
			args[i] = project
		}
	}
	env := []string{
		"HOME=" + home,
		"XDG_CONFIG_HOME=" + config,
		"LANG=C.UTF-8", "LC_ALL=C.UTF-8", "TZ=UTC",
	}
	command := exec.Command(binary, args...)
	command.Dir = project
	command.Env = env
	command.Stdin = bytes.NewReader(definition.Stdin)
	var stdout, stderr bytes.Buffer
	command.Stdout, command.Stderr = &stdout, &stderr
	err = command.Run()
	exit := 0
	if err != nil {
		exit = command.ProcessState.ExitCode()
		if exit < 0 {
			exit = 1
		}
	}
	return result{
		ID: definition.ID, Args: definition.Args, Stdin: string(definition.Stdin), Exit: exit,
		Stdout: normalize(root, stdout.String()), Stderr: normalize(root, stderr.String()), Files: files(root),
	}
}

func setupExisting(_root, _home, config, _project string) error {
	return write(filepath.Join(config, "symbrain", "profiles", "existing.toml"), []byte("[profile]\nname = \"existing\"\n"), 0o640)
}
func setupBoundGlobal(root, home, config, _project string) error {
	if err := write(filepath.Join(config, "symbrain", "profiles", "bound.toml"), []byte("[profile]\nname = \"bound\"\n"), 0o600); err != nil {
		return err
	}
	return write(filepath.Join(home, ".claude.json"), []byte(`{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","bound"]}}}`), 0o640)
}
func setupBoundProject(_root, _home, config, project string) error {
	if err := write(filepath.Join(config, "symbrain", "profiles", "existing.toml"), []byte("[profile]\nname = \"existing\"\n"), 0o600); err != nil {
		return err
	}
	return write(filepath.Join(project, ".mcp.json"), []byte(`{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","existing"]}}}`), 0o640)
}
func setupBoundProjectSymlink(_root, _home, config, project string) error {
	if err := write(filepath.Join(config, "symbrain", "profiles", "existing.toml"), []byte("[profile]\nname = \"existing\"\n"), 0o600); err != nil {
		return err
	}
	realProject := filepath.Join(filepath.Dir(project), "real-project")
	if err := os.MkdirAll(realProject, 0o700); err != nil {
		return err
	}
	if err := write(filepath.Join(realProject, ".mcp.json"), []byte(`{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","existing"]}}}`), 0o640); err != nil {
		return err
	}
	return os.Symlink("../real-project", filepath.Join(project, "project-link"))
}
func setupMalformedProfile(_root, _home, config, _project string) error {
	return write(filepath.Join(config, "symbrain", "profiles", "existing.toml"), []byte("[profile\n"), 0o600)
}
func setupMalformedHarness(root, home, config, project string) error {
	if err := setupBoundGlobal(root, home, config, project); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(home, ".claude.json"), []byte("{not valid json"), 0o640)
}
func setupSymlink(root, _home, config, _project string) error {
	profiles := filepath.Join(config, "symbrain", "profiles")
	if err := os.MkdirAll(profiles, 0o700); err != nil {
		return err
	}
	target := filepath.Join(root, "profile-target.toml")
	if err := os.WriteFile(target, []byte("target\n"), 0o600); err != nil {
		return err
	}
	return os.Symlink(target, filepath.Join(profiles, "existing.toml"))
}
func setupSymlinkProfilesRoot(_root, _home, config, _project string) error {
	profilesParent := filepath.Join(config, "symbrain")
	if err := os.MkdirAll(profilesParent, 0o700); err != nil {
		return err
	}
	outside := filepath.Join(config, "outside-profiles")
	if err := os.MkdirAll(outside, 0o700); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(outside, "existing.toml"), []byte("outside\n"), 0o600); err != nil {
		return err
	}
	return os.Symlink("../outside-profiles", filepath.Join(profilesParent, "profiles"))
}
func setupFIFO(_root, _home, config, _project string) error {
	profiles := filepath.Join(config, "symbrain", "profiles")
	if err := os.MkdirAll(profiles, 0o700); err != nil {
		return err
	}
	return mkfifo(filepath.Join(profiles, "existing.toml"))
}
func setupDirectory(_root, _home, config, _project string) error {
	return os.MkdirAll(filepath.Join(config, "symbrain", "profiles", "existing.toml"), 0o700)
}

func mkfifo(path string) error {
	if runtime.GOOS == "windows" {
		return fmt.Errorf("FIFO unsupported on Windows")
	}
	return exec.Command("mkfifo", path).Run()
}
func write(path string, data []byte, mode fs.FileMode) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	if err := os.WriteFile(path, data, mode); err != nil {
		return err
	}
	return os.Chmod(path, mode)
}
func files(root string) []fileState {
	var result []fileState
	_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || path == root {
			return nil
		}
		info, statErr := entry.Info()
		if statErr != nil {
			return nil
		}
		relative, relErr := filepath.Rel(root, path)
		if relErr != nil {
			return nil
		}
		state := fileState{Path: filepath.ToSlash(relative), Mode: uint32(info.Mode().Perm())}
		switch {
		case entry.Type()&fs.ModeSymlink != 0:
			state.Type = "symlink"
			if target, readErr := os.Readlink(path); readErr == nil {
				state.Bytes = []byte(target)
			}
		case entry.IsDir():
			state.Type = "dir"
		case info.Mode().IsRegular():
			state.Type = "file"
			state.Bytes, _ = os.ReadFile(path)
		default:
			state.Type = "other"
		}
		result = append(result, state)
		return nil
	})
	sort.Slice(result, func(i, j int) bool { return result[i].Path < result[j].Path })
	return result
}
func normalize(root, value string) string {
	return strings.ReplaceAll(value, filepath.ToSlash(root), "<root>")
}
func repoRoot() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		fatalf("locate oracle source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}
func gitRevision(root string) string {
	args := []string{"-C", root, "log", "-1", "--format=%H", "--"}
	args = append(args, oracleSourceFiles()...)
	output, err := exec.Command("git", args...).Output()
	if err != nil {
		fatalf("resolve Go revision: %v", err)
	}
	return strings.TrimSpace(string(output))
}
func oracleSourceFiles() []string {
	return []string{
		"cmd/symbrain/cmd_profile.go",
		"cmd/symbrain/cmd_profile_add.go",
		"cmd/symbrain/cmd_profile_list.go",
		"cmd/symbrain/cmd_profile_remove.go",
		"cmd/symbrain/cmd_profile_show.go",
		"internal/harness/document.go",
		"internal/harness/entry.go",
		"internal/harness/generic_document.go",
		"internal/harness/inventory.go",
		"internal/harness/inventory_read_other.go",
		"internal/harness/inventory_read_unix.go",
		"internal/harness/json_document.go",
		"internal/profile/remove_windows.go",
		"internal/safefs/doc.go",
		"internal/safefs/secure_other.go",
		"internal/safefs/secure_windows.go",
		"internal/harness/ordered_map.go",
		"internal/harness/registry.go",
		"internal/harness/toml_document.go",
		"internal/profile/profile.go",
		"internal/profile/remove_other.go",
		"internal/profile/remove_unix.go",
		"internal/xdg/xdg.go",
	}
}
func sourceHashes(root string) map[string]string {
	hashes := map[string]string{}
	for _, relative := range oracleSourceFiles() {
		hashes[relative] = fileSHA256(filepath.Join(root, filepath.FromSlash(relative)))
	}
	return hashes
}
func fileSHA256(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		fatalf("read %s: %v", path, err)
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
