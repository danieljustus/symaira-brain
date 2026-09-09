package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/instructions"
)

const rollbackBaseline = "4b6be3b"

type Oracle struct {
	SchemaVersion    int          `json:"schema_version"`
	RollbackBaseline string       `json:"rollback_baseline"`
	GoToolchain      string       `json:"go_toolchain"`
	GeneratorSHA256  string       `json:"generator_sha256"`
	Provenance       []Provenance `json:"provenance"`
	Cases            []Case       `json:"cases"`
}

type Provenance struct {
	Path            string `json:"path"`
	SHA256          string `json:"sha256"`
	RollbackSHA256  string `json:"rollback_sha256"`
	RollbackPresent bool   `json:"rollback_present"`
}

type Case struct {
	ID                  string `json:"id"`
	Operation           string `json:"operation"`
	Existing            []byte `json:"existing,omitempty"`
	Content             []byte `json:"content,omitempty"`
	Expected            []byte `json:"expected,omitempty"`
	Global              []byte `json:"global,omitempty"`
	Project             []byte `json:"project,omitempty"`
	GlobalPresent       bool   `json:"global_present,omitempty"`
	ProjectPresent      bool   `json:"project_present,omitempty"`
	ProjectDir          string `json:"project_dir,omitempty"`
	ExpectedGlobalPath  string `json:"expected_global_path,omitempty"`
	ExpectedProjectPath string `json:"expected_project_path,omitempty"`
	XDGConfigHome       string `json:"xdg_config_home,omitempty"`
	ExpectedXDGGlobal   string `json:"expected_xdg_global_path,omitempty"`
	Verdict             string `json:"verdict,omitempty"`
	Error               string `json:"error,omitempty"`
	PlatformEvidence    string `json:"platform_evidence,omitempty"`
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs from the fixture")
	output := flag.String("output", "rust/symbrain-instructions/tests/fixtures/oracle_expectations.json", "fixture path")
	flag.Parse()

	root := repositoryRoot()
	var oracle Oracle
	withIsolatedEnvironment(func() {
		oracle = buildOracle(root)
	})
	data, err := json.MarshalIndent(oracle, "", "  ")
	if err != nil {
		fatalf("marshal oracle: %v", err)
	}
	data = append(data, '\n')

	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil {
			fatalf("read %s: %v", *output, err)
		}
		if !bytes.Equal(existing, data) {
			fatalf("%s is out of date; run go run ./scripts/instructions-oracle", *output)
		}
		fmt.Printf("PASS: instructions oracle deterministic check passed (%d cases)\n", len(oracle.Cases))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o700); err != nil {
		fatalf("mkdir %s: %v", filepath.Dir(*output), err)
	}
	if err := os.WriteFile(*output, data, 0o600); err != nil {
		fatalf("write %s: %v", *output, err)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(oracle.Cases))
}

func buildOracle(root string) Oracle {
	golden := func(name string) []byte {
		data, err := os.ReadFile(filepath.Join(root, "internal", "instructions", "testdata", "block", name+".golden"))
		if err != nil {
			fatalf("read golden %s: %v", name, err)
		}
		return data
	}
	block := []Case{
		renderCase("render_idempotent", "", "some managed content\nwith multiple lines\n", nil),
		renderCase("render_preserves_content", "# My Config\n\nSome user text.\n"+instructions.BeginMarker+"\nold content\n"+instructions.EndMarker+"\n\nFooter here.\n", "new managed content\n", nil),
		renderCase("render_appends", "# Header\n\nSome content.\n", "block content\n", nil),
		renderCase("render_appends_without_trailing_newline", "no trailing newline", "block\n", nil),
		renderCase("render_empty_target", "", "hello\n", nil),
		renderCase("render_replaces_existing", "before\n"+instructions.BeginMarker+"\nold\n"+instructions.EndMarker+"\nafter\n", "updated content\n", nil),
		renderCase("render_begin_without_end", "before\n"+instructions.BeginMarker+"\nold stuff\n", "new\n", nil),
		renderCase("render_crlf", "before\r\n"+instructions.BeginMarker+"\r\nold\r\n"+instructions.EndMarker+"\r\nafter\r\n", "content\r\n", nil),
		renderCase("render_crlf_idempotent", "", "content\r\n", nil),
		renderCase("render_multiple_blocks", instructions.BeginMarker+"\nfirst\n"+instructions.EndMarker+"\n"+instructions.BeginMarker+"\nsecond\n"+instructions.EndMarker+"\n", "final\n", nil),
		renderCase("golden_fresh_file", "", "Instructions content.\nMore lines.\n", golden("fresh_file")),
		renderCase("golden_existing_with_user_content", "# User Header\n\nUser content before block.\n"+instructions.BeginMarker+"\nOld content.\n"+instructions.EndMarker+"\n\nUser footer.\n", "Updated managed block.\n", golden("existing_with_user_content")),
		renderCase("golden_file_without_markers", "# My Config\n\nSome config values.\n", "Managed block added.\n", golden("file_without_markers")),
		renderCase("golden_crlf_file", "header\r\n"+instructions.BeginMarker+"\r\nold\r\n"+instructions.EndMarker+"\r\nfooter\r\n", "content\r\n", golden("crlf_file")),
		renderCase("golden_no_trailing_newline", "no-newline", "block\n", golden("no_trailing_newline")),
		renderBytesCase("render_non_utf8", []byte("prefix\xff\x00"), []byte("body\xfe\n"), nil),
		renderBytesCase("render_prefix_code_non_utf8", []byte("prefix\xff\x00"), []byte("body\xfe\n"+instructions.EscapeSentinel+instructions.BeginMarker+instructions.EndMarker), nil),
		renderCase("render_reserved_markers", "prefix\n", "literal "+instructions.BeginMarker+" and "+instructions.EndMarker+"\n", nil),
		renderCase("render_escaped_marker_collision", "prefix\n", "literal "+instructions.EscapedBeginMarker+" and "+instructions.EscapedEndMarker+"\n", nil),
		renderCase("render_escape_sentinel", "prefix\n", instructions.EscapeSentinel+" "+instructions.EscapedBeginMarker+" "+instructions.EscapedEndMarker+"\n", nil),
		renderCase("render_double_begin_vs_escape_token", "prefix\n", instructions.BeginMarker+instructions.BeginMarker, nil),
		renderCase("render_existing_escape_tokens", "prefix\n", instructions.EscapedBeginMarker+" "+instructions.EscapedEndMarker, nil),
		renderCase("render_ordinary_content", "prefix\n", "human-visible <!-- symbrain: ordinary -->\n", nil),
	}

	source := []Case{
		sourceCase("source_global_only", []byte("global instructions\n"), nil, true, false),
		sourceCase("source_project_appended", []byte("global\n"), []byte("project\n"), true, true),
		sourceCase("source_neither_exists", nil, nil, false, false),
		sourceCase("source_project_only", nil, []byte("project only\n"), false, true),
		sourceErrorCase("source_symlink_rejected", "unix_runtime"),
		sourceErrorCase("source_directory_rejected", "portable_runtime"),
		sourceErrorCase("source_per_file_limit_rejected", "portable_runtime"),
		sourceErrorCase("source_merged_limit_rejected", "portable_runtime"),
	}
	pathCase := newSourceCase()

	cases := append(block, append(source, pathCase)...)
	return Oracle{
		SchemaVersion:    1,
		RollbackBaseline: resolvedBaseline(root),
		GoToolchain:      goToolchain(root),
		GeneratorSHA256:  fileSHA256(filepath.Join(root, "scripts", "instructions-oracle", "main.go")),
		Provenance:       provenance(root),
		Cases:            cases,
	}
}

func renderCase(id, existing, content string, expected []byte) Case {
	return renderBytesCase(id, []byte(existing), []byte(content), expected)
}

func renderBytesCase(id string, existing, content, expected []byte) Case {
	if expected == nil {
		expected = []byte(instructions.Render(string(existing), string(content)))
	}
	return Case{ID: id, Operation: "render", Existing: existing, Content: content, Expected: expected}
}

func newSourceCase() Case {
	oldHome, hadHome := os.LookupEnv("HOME")
	oldXDG, hadXDG := os.LookupEnv("XDG_CONFIG_HOME")
	defer func() {
		if hadHome {
			_ = os.Setenv("HOME", oldHome)
		} else {
			_ = os.Unsetenv("HOME")
		}
		if hadXDG {
			_ = os.Setenv("XDG_CONFIG_HOME", oldXDG)
		} else {
			_ = os.Unsetenv("XDG_CONFIG_HOME")
		}
	}()
	if err := os.Setenv("HOME", "/oracle-home"); err != nil {
		fatalf("set source path home: %v", err)
	}
	if err := os.Unsetenv("XDG_CONFIG_HOME"); err != nil {
		fatalf("unset source path XDG config: %v", err)
	}
	defaultSource := instructions.NewSource("/oracle-project")
	if err := os.Setenv("XDG_CONFIG_HOME", "/oracle-config"); err != nil {
		fatalf("set source path XDG config: %v", err)
	}
	xdgSource := instructions.NewSource("/oracle-project")
	return Case{
		ID:                  "source_paths",
		Operation:           "source_paths",
		ProjectDir:          "/oracle-project",
		XDGConfigHome:       "/oracle-config",
		ExpectedGlobalPath:  defaultSource.GlobalPath,
		ExpectedProjectPath: defaultSource.ProjectPath,
		ExpectedXDGGlobal:   xdgSource.GlobalPath,
	}
}

func sourceCase(id string, global, project []byte, globalPresent, projectPresent bool) Case {
	root, err := os.MkdirTemp("", "symbrain-instructions-oracle-")
	if err != nil {
		fatalf("create isolated source root: %v", err)
	}
	defer os.RemoveAll(root)
	globalPath := filepath.Join(root, "global", "instructions.md")
	projectPath := filepath.Join(root, "project", ".symbrain", "instructions.md")
	if globalPresent {
		writeIsolated(globalPath, global)
	}
	if projectPresent {
		writeIsolated(projectPath, project)
	}
	s := &instructions.Source{GlobalPath: globalPath, ProjectPath: projectPath}
	merged, err := s.Content()
	if err != nil {
		fatalf("source case %s: %v", id, err)
	}
	return Case{ID: id, Operation: "source", Global: global, Project: project, Expected: []byte(merged), GlobalPresent: globalPresent, ProjectPresent: projectPresent}
}

func sourceErrorCase(id, platformEvidence string) Case {
	result := Case{
		ID:               id,
		Operation:        "source_verdict",
		Verdict:          "error",
		Error:            sourceErrorExpected(id),
		PlatformEvidence: platformEvidence,
	}
	if platformEvidence == "unix_runtime" && !isUnix() {
		// The fixture records the Unix-only contract even when regenerated on a
		// platform that cannot exercise the no-follow primitive.
		return result
	}

	root, err := os.MkdirTemp("", "symbrain-instructions-verdict-")
	if err != nil {
		fatalf("create source verdict root: %v", err)
	}
	defer os.RemoveAll(root)
	globalPath := filepath.Join(root, "global")
	projectPath := filepath.Join(root, "project")
	switch id {
	case "source_symlink_rejected":
		outside := filepath.Join(root, "outside")
		writeIsolated(outside, []byte("outside"))
		if err := os.Symlink(outside, globalPath); err != nil {
			fatalf("create source symlink: %v", err)
		}
	case "source_directory_rejected":
		if err := os.Mkdir(globalPath, 0o700); err != nil {
			fatalf("create source directory: %v", err)
		}
	case "source_per_file_limit_rejected":
		writeIsolated(globalPath, make([]byte, instructions.MaxSourceFileBytes+1))
	case "source_merged_limit_rejected":
		writeIsolated(globalPath, make([]byte, instructions.MaxSourceFileBytes))
		writeIsolated(projectPath, make([]byte, instructions.MaxSourceTotalBytes-instructions.MaxSourceFileBytes+1))
	default:
		fatalf("unknown source verdict case %s", id)
	}
	_, err = (&instructions.Source{GlobalPath: globalPath, ProjectPath: projectPath}).Content()
	if err == nil {
		fatalf("source verdict %s did not return an error", id)
	}
	normalized := filepath.ToSlash(strings.ReplaceAll(err.Error(), root, "<root>"))
	if normalized != result.Error {
		fatalf("source verdict %s: normalized error %q differs from expected %q", id, normalized, result.Error)
	}
	return result
}

func sourceErrorExpected(id string) string {
	switch id {
	case "source_symlink_rejected":
		return "instructions: read <root>/global: global is a symlink"
	case "source_directory_rejected":
		return "instructions: read <root>/global: global is not a regular file"
	case "source_per_file_limit_rejected":
		return "instructions: read <root>/global: global exceeds maximum size of 1048576 bytes"
	case "source_merged_limit_rejected":
		return "instructions: merged source exceeds maximum size of 1572864 bytes"
	default:
		fatalf("unknown source error case %s", id)
		return ""
	}
}

func isUnix() bool {
	switch runtime.GOOS {
	case "darwin", "dragonfly", "freebsd", "linux", "netbsd", "openbsd", "solaris":
		return true
	default:
		return false
	}
}

func provenance(root string) []Provenance {
	paths := []string{
		"internal/instructions/block.go",
		"internal/instructions/source.go",
		"internal/instructions/doc.go",
		"internal/instructions/block_test.go",
		"internal/instructions/source_read_other.go",
		"internal/instructions/source_read_unix.go",
		"internal/instructions/source_read_windows.go",
	}
	for _, name := range []string{"crlf_file", "existing_with_user_content", "file_without_markers", "fresh_file", "no_trailing_newline"} {
		paths = append(paths, filepath.ToSlash(filepath.Join("internal", "instructions", "testdata", "block", name+".golden")))
	}
	sort.Strings(paths)
	result := make([]Provenance, 0, len(paths))
	for _, rel := range paths {
		current, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(rel)))
		if err != nil {
			fatalf("read provenance %s: %v", rel, err)
		}
		data, present := gitShow(root, rollbackBaseline, rel)
		result = append(result, Provenance{
			Path:            rel,
			SHA256:          sha256Hex(current),
			RollbackSHA256:  sha256Hex(data),
			RollbackPresent: present,
		})
	}
	return result
}

func fileSHA256(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		fatalf("read hash input %s: %v", path, err)
	}
	return sha256Hex(data)
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func gitShow(root, revision, rel string) ([]byte, bool) {
	cmd := exec.Command("git", "-C", root, "show", revision+":"+filepath.ToSlash(rel))
	data, err := cmd.Output()
	if err != nil {
		return nil, false
	}
	return data, true
}

func goToolchain(root string) string {
	data, err := os.ReadFile(filepath.Join(root, "go.mod"))
	if err != nil {
		fatalf("read go.mod: %v", err)
	}
	for _, line := range strings.Split(string(data), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 2 && fields[0] == "go" {
			return "go" + fields[1]
		}
	}
	fatalf("go.mod does not declare a go version")
	return ""
}

func repositoryRoot() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		fatalf("locate oracle source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}

func resolvedBaseline(root string) string {
	cmd := exec.Command("git", "-C", root, "rev-parse", rollbackBaseline)
	output, err := cmd.Output()
	if err != nil {
		fatalf("resolve rollback baseline: %v", err)
	}
	return string(bytes.TrimSpace(output))
}

func writeIsolated(path string, data []byte) {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		fatalf("mkdir isolated source: %v", err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		fatalf("write isolated source: %v", err)
	}
}

func withIsolatedEnvironment(fn func()) {
	root, err := os.MkdirTemp("", "symbrain-instructions-env-")
	if err != nil {
		fatalf("create isolated environment: %v", err)
	}
	defer os.RemoveAll(root)

	names := []string{"HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "LC_ALL", "LANG", "TZ"}
	original := make(map[string]string, len(names))
	present := make(map[string]bool, len(names))
	for _, name := range names {
		original[name], present[name] = os.LookupEnv(name)
	}
	defer func() {
		for _, name := range names {
			if present[name] {
				_ = os.Setenv(name, original[name])
			} else {
				_ = os.Unsetenv(name)
			}
		}
	}()
	values := map[string]string{
		"HOME":            filepath.Join(root, "home"),
		"XDG_CONFIG_HOME": filepath.Join(root, "config"),
		"XDG_DATA_HOME":   filepath.Join(root, "data"),
		"XDG_CACHE_HOME":  filepath.Join(root, "cache"),
		"LC_ALL":          "C.UTF-8",
		"LANG":            "C.UTF-8",
		"TZ":              "UTC",
	}
	for name, value := range values {
		if err := os.Setenv(name, value); err != nil {
			fatalf("set isolated %s: %v", name, err)
		}
	}
	fn()
}

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
