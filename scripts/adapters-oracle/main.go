// Command adapters-oracle freezes the Go instruction-adapter contract.
// It uses the production registry, adapter implementations, and instruction
// renderer; the Rust crate consumes the resulting language-neutral bytes.
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

	"github.com/danieljustus/symaira-brain/internal/adapter"
	"github.com/danieljustus/symaira-brain/internal/harness"
)

const rollbackBaseline = "4b6be3b"
const projectDir = "/oracle-project"

type provenance struct {
	Path            string `json:"path"`
	SHA256          string `json:"sha256"`
	RollbackSHA256  string `json:"rollback_sha256"`
	RollbackPresent bool   `json:"rollback_present"`
}

type testCase struct {
	ID         string `json:"id"`
	Harness    string `json:"harness"`
	Operation  string `json:"operation"`
	ProjectDir string `json:"project_dir,omitempty"`
	TargetPath string `json:"target_path,omitempty"`
	Existing   []byte `json:"existing,omitempty"`
	Content    []byte `json:"content,omitempty"`
	Expected   []byte `json:"expected,omitempty"`
	Skipped    bool   `json:"skipped,omitempty"`
}

type oracle struct {
	SchemaVersion    int          `json:"schema_version"`
	RollbackBaseline string       `json:"rollback_baseline"`
	GoToolchain      string       `json:"go_toolchain"`
	GeneratorSHA256  string       `json:"generator_sha256"`
	Provenance       []provenance `json:"provenance"`
	Cases            []testCase   `json:"cases"`
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-adapter/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()

	root := repositoryRoot()
	generated := buildOracle(root)
	data, err := json.MarshalIndent(generated, "", "  ")
	if err != nil {
		fatalf("marshal oracle: %v", err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fatalf("%s is out of date; run go run ./scripts/adapters-oracle", *output)
		}
		fmt.Printf("PASS: adapters oracle deterministic check passed (%d cases)\n", len(generated.Cases))
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

func buildOracle(root string) oracle {
	targets := adapter.TargetsForHarnesses()
	content := []byte("managed instructions\n")
	result := oracle{
		SchemaVersion:    1,
		RollbackBaseline: resolvedBaseline(root),
		GoToolchain:      goToolchain(root),
		GeneratorSHA256:  fileSHA256(filepath.Join(root, "scripts", "adapters-oracle", "main.go")),
		Provenance:       provenanceFor(root),
	}

	for _, registered := range harness.All {
		name := string(registered.Name)
		target, ok := targets[name]
		if !ok {
			result.Cases = append(result.Cases, testCase{
				ID:        "skip_" + name,
				Harness:   name,
				Operation: "skip",
				Skipped:   true,
			})
			continue
		}
		path := filepath.ToSlash(filepath.Join(projectDir, target.Dir, target.Filename))
		result.Cases = append(result.Cases,
			renderCase(name+"_fresh", name, target, nil, content, path),
			renderCase(name+"_existing_without_markers", name, target,
				[]byte("existing prefix\nexisting suffix\n"),
				[]byte("appended managed\n"), path),
			renderCase(name+"_existing_user_prefix_suffix", name, target,
				[]byte("user prefix\n"+"<!-- symbrain:begin -->\nold\n<!-- symbrain:end -->\nuser suffix\n"),
				[]byte("updated managed\n"), path),
			renderCase(name+"_malformed_non_utf8_marker_content", name, target,
				[]byte("prefix\xff\x00\n<!-- symbrain:begin -->\nold\n"),
				[]byte("body\xfe\n<!-- symbrain:begin --> and <!-- symbrain:end -->\n"), path),
			renderCase(name+"_crlf", name, target,
				[]byte("header\r\n<!-- symbrain:begin -->\r\nold\r\n<!-- symbrain:end -->\r\nfooter\r\n"),
				[]byte("content\r\n"), path),
		)
	}
	return result
}

func renderCase(id, name string, target adapter.Target, existing, content []byte, path string) testCase {
	return testCase{
		ID:         id,
		Harness:    name,
		Operation:  "render",
		ProjectDir: projectDir,
		TargetPath: path,
		Existing:   existing,
		Content:    content,
		Expected:   []byte(target.Render(string(existing), string(content), projectDir)),
	}
}

func provenanceFor(root string) []provenance {
	paths := []string{
		"internal/adapter/adapter.go",
		"internal/adapter/agents.go",
		"internal/adapter/antigravity.go",
		"internal/adapter/claude.go",
		"internal/adapter/cursor.go",
		"internal/harness/registry.go",
		"internal/instructions/block.go",
	}
	sort.Strings(paths)
	result := make([]provenance, 0, len(paths))
	for _, rel := range paths {
		current, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(rel)))
		if err != nil {
			fatalf("read provenance %s: %v", rel, err)
		}
		rollback, present := gitShow(root, rollbackBaseline, rel)
		result = append(result, provenance{
			Path:            rel,
			SHA256:          sha256Hex(current),
			RollbackSHA256:  sha256Hex(rollback),
			RollbackPresent: present,
		})
	}
	return result
}

func repositoryRoot() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		fatalf("locate oracle source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}

func resolvedBaseline(root string) string {
	output, err := exec.Command("git", "-C", root, "rev-parse", rollbackBaseline).Output()
	if err != nil {
		fatalf("resolve rollback baseline: %v", err)
	}
	return strings.TrimSpace(string(output))
}

func gitShow(root, revision, rel string) ([]byte, bool) {
	data, err := exec.Command("git", "-C", root, "show", revision+":"+filepath.ToSlash(rel)).Output()
	if err != nil {
		return nil, false
	}
	return data, true
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

func fatalf(format string, args ...any) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
