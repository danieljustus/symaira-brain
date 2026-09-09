// Command install-oracle freezes the Go install/uninstall contract.
//
// Every case builds and runs the production Go CLI in a fresh HOME, XDG tree,
// and project directory. The generated fixture is therefore source-bound while
// remaining safe to run on a developer machine with real configuration files.
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
	"regexp"
	"runtime"
	"sort"
	"strings"
	"time"
)

var timestampPattern = regexp.MustCompile(`\.bak\.[0-9]{8}T[0-9]{6}Z(\.[0-9]+)?`)

type fileState struct {
	Path  string `json:"path"`
	Type  string `json:"type"`
	Mode  uint32 `json:"mode"`
	Bytes []byte `json:"bytes,omitempty"`
}

type result struct {
	ID     string      `json:"id"`
	Args   []string    `json:"args"`
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
	ID    string
	Args  []string
	Env   map[string]string
	Setup func(root, home, config, project string) error
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-harness/tests/fixtures/install_oracle_"+runtime.GOOS+".json", "output path")
	flag.Parse()
	root := repoRoot()
	generated := generate(root)
	data, err := json.MarshalIndent(generated, "", "  ")
	if err != nil {
		fatalf("marshal oracle: %v", err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			diagnoseMismatch(existing, generated)
			fatalf("%s is out of date; run go run ./scripts/install-oracle", *output)
		}
		fmt.Printf("PASS: install oracle deterministic check passed (%d cases)\n", len(generated.Cases))
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

func diagnoseMismatch(existing []byte, generated oracle) {
	var expected oracle
	if err := json.Unmarshal(existing, &expected); err != nil {
		fmt.Fprintf(os.Stderr, "install oracle diagnostic: existing fixture is not valid JSON: %v\n", err)
		return
	}
	for _, field := range []struct {
		name string
		old  any
		new  any
	}{
		{"schema_version", expected.SchemaVersion, generated.SchemaVersion},
		{"go_revision", expected.GoRevision, generated.GoRevision},
		{"generator_sha256", expected.GeneratorSHA256, generated.GeneratorSHA256},
		{"go_sources", expected.GoSources, generated.GoSources},
	} {
		if !reflect.DeepEqual(field.old, field.new) {
			fmt.Fprintf(os.Stderr, "install oracle diagnostic: %s differs\n", field.name)
		}
	}
	if len(expected.Cases) != len(generated.Cases) {
		fmt.Fprintf(os.Stderr, "install oracle diagnostic: case count old=%d new=%d\n", len(expected.Cases), len(generated.Cases))
	}
	for i := 0; i < len(expected.Cases) && i < len(generated.Cases); i++ {
		oldCase, newCase := expected.Cases[i], generated.Cases[i]
		if reflect.DeepEqual(oldCase, newCase) {
			continue
		}
		fmt.Fprintf(os.Stderr, "install oracle diagnostic: case %q differs\n", newCase.ID)
		if oldCase.Exit != newCase.Exit {
			fmt.Fprintf(os.Stderr, "  exit old=%d new=%d\n", oldCase.Exit, newCase.Exit)
		}
		if oldCase.Stdout != newCase.Stdout {
			fmt.Fprintf(os.Stderr, "  stdout differs old=%q new=%q\n", oldCase.Stdout, newCase.Stdout)
		}
		if oldCase.Stderr != newCase.Stderr {
			fmt.Fprintf(os.Stderr, "  stderr differs old=%q new=%q\n", oldCase.Stderr, newCase.Stderr)
		}
		if !reflect.DeepEqual(oldCase.Files, newCase.Files) {
			fmt.Fprintf(os.Stderr, "  files differ old_count=%d new_count=%d\n", len(oldCase.Files), len(newCase.Files))
			for j := 0; j < len(oldCase.Files) && j < len(newCase.Files); j++ {
				oldFile, newFile := oldCase.Files[j], newCase.Files[j]
				if reflect.DeepEqual(oldFile, newFile) {
					continue
				}
				fmt.Fprintf(os.Stderr, "  file[%d] old=(%s,%s,%d,%d bytes) new=(%s,%s,%d,%d bytes)\n",
					j, oldFile.Path, oldFile.Type, oldFile.Mode, len(oldFile.Bytes),
					newFile.Path, newFile.Type, newFile.Mode, len(newFile.Bytes))
			}
		}
	}
}

func generate(root string) oracle {
	goBinary, err := os.CreateTemp("", "symbrain-go-install-oracle-*")
	if err != nil {
		fatalf("create Go binary: %v", err)
	}
	binary := goBinary.Name()
	_ = goBinary.Close()
	defer os.Remove(binary)
	cmd := exec.Command("go", "build", "-o", binary, "./cmd/symbrain")
	cmd.Dir = root
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		fatalf("build Go oracle: %v\n%s", err, output)
	}

	result := oracle{
		SchemaVersion:   1,
		GoRevision:      gitRevision(root),
		GeneratorSHA256: fileSHA256(filepath.Join(root, "scripts", "install-oracle", "main.go")),
		GoSources:       sourceHashes(root),
	}
	for _, definition := range definitions() {
		result.Cases = append(result.Cases, runCase(binary, definition))
	}
	return result
}

func definitions() []caseDef {
	return []caseDef{
		{ID: "fresh_entry_bytes_and_modes", Args: []string{"install", "--harness", "cursor", "--profile", "personal"}},
		{ID: "fresh_entry_claude", Args: []string{"install", "--harness", "claude", "--profile", "personal"}},
		{ID: "fresh_entry_claude_desktop", Args: []string{"install", "--harness", "claude-desktop", "--profile", "personal"}},
		{ID: "fresh_entry_opencode", Args: []string{"install", "--harness", "opencode", "--profile", "personal"}},
		{ID: "fresh_entry_codex", Args: []string{"install", "--harness", "codex", "--profile", "personal"}},
		{ID: "fresh_entry_antigravity", Args: []string{"install", "--harness", "antigravity", "--profile", "personal"}},
		{ID: "default_profile", Args: []string{"install", "--harness", "claude"}, Setup: setupDefaultProfile},
		{ID: "default_profile_env_override", Args: []string{"install", "--harness", "claude"}, Env: map[string]string{"SYMBRAIN_DEFAULT_PROFILE": "restricted"}, Setup: setupDefaultProfile},
		{ID: "malformed_global_config", Args: []string{"install", "--harness", "claude"}, Setup: setupMalformedGlobal},
		{ID: "missing_profile_no_default", Args: []string{"install", "--harness", "cursor"}},
		{ID: "malformed_refuses_before_backup", Args: []string{"install", "--harness", "claude", "--profile", "personal"}, Setup: setupMalformed},
		{ID: "superseded_basenames_keep_vault", Args: []string{"install", "--harness", "cursor", "--profile", "personal"}, Setup: setupSuperseded},
		{ID: "keep_superseded", Args: []string{"install", "--harness", "cursor", "--profile", "personal", "--keep-superseded"}, Setup: setupSuperseded},
		{ID: "claude_project_constraint", Args: []string{"install", "--harness", "claude", "--profile", "personal", "--project", "PROJECT"}},
		{ID: "unsupported_project_constraint", Args: []string{"install", "--harness", "cursor", "--profile", "personal", "--project", "PROJECT"}},
		{ID: "dry_run", Args: []string{"install", "--harness", "cursor", "--profile", "personal", "--dry-run"}, Setup: setupExistingCursor},
		{ID: "foreign_named_symbrain_uninstall_noop", Args: []string{"uninstall", "--harness", "cursor"}, Setup: setupForeign},
		{ID: "missing_config_uninstall_noop", Args: []string{"uninstall", "--harness", "cursor"}},
		{ID: "absent_entry_uninstall_noop", Args: []string{"uninstall", "--harness", "cursor"}, Setup: setupExistingCursor},
		{ID: "uninstall_removes_symbrain", Args: []string{"uninstall", "--harness", "cursor"}, Setup: setupInstalled},
		{ID: "backup_bytes_mode_timestamp_shape", Args: []string{"install", "--harness", "cursor", "--profile", "personal"}, Setup: setupBackupMode},
	}
}

func runCase(binary string, definition caseDef) result {
	root, err := os.MkdirTemp("", "symbrain-install-oracle-*")
	if err != nil {
		fatalf("create case root: %v", err)
	}
	canonicalRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		defer os.RemoveAll(root)
		fatalf("canonicalize case root: %v", err)
	}
	defer os.RemoveAll(root)
	root = canonicalRoot
	home := filepath.Join(root, "home")
	config := filepath.Join(root, "config")
	project := filepath.Join(root, "project")
	for _, path := range []string{home, config, filepath.Join(root, "data"), filepath.Join(root, "cache"), project} {
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
		"XDG_DATA_HOME=" + filepath.Join(root, "data"),
		"XDG_CACHE_HOME=" + filepath.Join(root, "cache"),
		"XDG_STATE_HOME=" + filepath.Join(root, "state"),
		"LANG=C.UTF-8", "LC_ALL=C.UTF-8", "TZ=UTC",
	}
	for key, value := range definition.Env {
		env = append(env, key+"="+value)
	}
	command := exec.Command(binary, args...)
	command.Dir = project
	command.Env = env
	stdout, stderr := &bytes.Buffer{}, &bytes.Buffer{}
	command.Stdout, command.Stderr = stdout, stderr
	err = command.Run()
	exit := 0
	if err != nil {
		if status, ok := command.ProcessState.Sys().(interface{ ExitStatus() int }); ok {
			exit = status.ExitStatus()
		} else if command.ProcessState.ExitCode() >= 0 {
			exit = command.ProcessState.ExitCode()
		} else {
			exit = 1
		}
	}
	return result{
		ID: definition.ID, Args: definition.Args, Exit: exit,
		Stdout: normalize(root, stdout.String()), Stderr: normalize(root, stderr.String()),
		Files: files(root),
	}
}

func setupDefaultProfile(_root, _home, config, _project string) error {
	return write(filepath.Join(config, "symbrain", "config.toml"), []byte("default_profile = \"personal\"\n"), 0o600)
}
func setupMalformedGlobal(_root, _home, config, _project string) error {
	return write(filepath.Join(config, "symbrain", "config.toml"), []byte("invalid = [ unterminated toml\n"), 0o600)
}
func setupMalformed(_root, home, _config, _project string) error {
	return write(filepath.Join(home, ".claude.json"), []byte("{not valid json"), 0o600)
}
func setupSuperseded(_root, home, _config, _project string) error {
	return write(filepath.Join(home, ".cursor", "mcp.json"), []byte(`{"mcpServers":{"old-memory":{"command":"/opt/bin/symmemory"},"old-skills":{"command":"symskills"},"vault":{"command":"symvault"},"other":{"command":"other"}}}`), 0o640)
}
func setupExistingCursor(_root, home, _config, _project string) error {
	return write(filepath.Join(home, ".cursor", "mcp.json"), []byte(`{"mcpServers":{"other":{"command":"other"}}}`), 0o640)
}
func setupForeign(_root, home, _config, _project string) error {
	return write(filepath.Join(home, ".cursor", "mcp.json"), []byte(`{"mcpServers":{"symbrain":{"command":"not-symbrain","args":["x"]}}}`), 0o640)
}
func setupInstalled(_root, home, _config, _project string) error {
	return write(filepath.Join(home, ".cursor", "mcp.json"), []byte(`{"mcpServers":{"other":{"command":"other"},"symbrain":{"command":"symbrain","args":["mcp","--profile","personal"]}}}`), 0o640)
}
func setupBackupMode(_root, home, _config, _project string) error {
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := write(path, []byte(`{"mcpServers":{"other":{"command":"other"}}}`), 0o640); err != nil {
		return err
	}
	return write(path+".bak."+time.Now().UTC().Format("20060102T150405Z"), []byte("reserved rollback"), 0o600)
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
		state := fileState{Path: normalize(root, filepath.ToSlash(relative)), Mode: uint32(info.Mode().Perm())}
		if entry.IsDir() {
			state.Type = "dir"
		} else if info.Mode().IsRegular() {
			state.Type = "file"
			state.Bytes, _ = os.ReadFile(path)
		} else {
			state.Type = "other"
		}
		result = append(result, state)
		return nil
	})
	sort.Slice(result, func(i, j int) bool { return result[i].Path < result[j].Path })
	return result
}

func normalize(root, value string) string {
	value = strings.ReplaceAll(value, filepath.ToSlash(root), "<root>")
	return timestampPattern.ReplaceAllString(value, ".bak.<timestamp>")
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
		"cmd/symbrain/cmd_install.go", "cmd/symbrain/cmd_uninstall.go",
		"internal/harness/document.go", "internal/harness/entry.go", "internal/harness/backup.go", "internal/harness/registry.go",
		"internal/skills/install/install.go", "internal/skills/install/base.go", "internal/skills/install/status.go",
		"internal/skills/install/classify.go", "internal/skills/install/pull.go", "internal/skills/install/sync.go",
		"internal/skills/install/pull_lock.go",
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
func fatalf(format string, args ...any) { fmt.Fprintf(os.Stderr, format+"\n", args...); os.Exit(1) }
