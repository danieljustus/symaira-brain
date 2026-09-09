// Command skills-install-oracle freezes the Go skills install/status contract.
// It imports the production skills packages directly; it is not a mock oracle.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

const timestamp = "<timestamp>"

type fileState struct {
	Path  string `json:"path"`
	Type  string `json:"type"`
	Mode  uint32 `json:"mode"`
	Bytes []byte `json:"bytes,omitempty"`
}

type statusState struct {
	Target      string              `json:"target"`
	Name        string              `json:"name"`
	Path        string              `json:"path"`
	Status      install.StatusKind  `json:"status"`
	Mode        install.Mode        `json:"mode,omitempty"`
	InstalledAt string              `json:"installed_at,omitempty"`
	SourceHash  string              `json:"source_hash,omitempty"`
	Error       string              `json:"error,omitempty"`
	Drift       []install.FileDrift `json:"drift,omitempty"`
}
type expected struct {
	Action    string        `json:"action,omitempty"`
	Mode      install.Mode  `json:"mode,omitempty"`
	Statuses  []statusState `json:"statuses"`
	Artifacts []fileState   `json:"artifacts"`
}
type oracle struct {
	SchemaVersion   int               `json:"schema_version"`
	GoRevision      string            `json:"go_revision"`
	GeneratorSHA256 string            `json:"generator_sha256"`
	GoSources       map[string]string `json:"go_sources"`
	Cases           []struct {
		ID       string   `json:"id"`
		Body     string   `json:"body"`
		Mutation string   `json:"mutation,omitempty"`
		Expected expected `json:"expected"`
	} `json:"cases"`
}
type caseDef struct{ ID, Body, Mutation string }

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-skills/tests/fixtures/install_status_oracle.json", "fixture path")
	flag.Parse()
	root := repoRoot()
	got := generate(root)
	data, err := json.MarshalIndent(got, "", "  ")
	if err != nil {
		fatalf("marshal oracle: %v", err)
	}
	data = append(data, '\n')
	if *check {
		want, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(want, data) {
			fatalf("%s is out of date; run go run ./scripts/skills-install-oracle", *output)
		}
		fmt.Printf("PASS: skills install/status oracle deterministic check passed (%d cases)\n", len(got.Cases))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fatalf("mkdir fixture: %v", err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fatalf("write fixture: %v", err)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(got.Cases))
}

func generate(root string) oracle {
	out := oracle{SchemaVersion: 1, GoRevision: "working-tree", GeneratorSHA256: fileSHA256(filepath.Join(root, "scripts", "skills-install-oracle", "main.go")), GoSources: sourceHashes(root)}
	for _, def := range definitions() {
		out.Cases = append(out.Cases, struct {
			ID       string   `json:"id"`
			Body     string   `json:"body"`
			Mutation string   `json:"mutation,omitempty"`
			Expected expected `json:"expected"`
		}{def.ID, def.Body, def.Mutation, runCase(def)})
	}
	return out
}
func definitions() []caseDef {
	return []caseDef{
		{"fresh_copy_custom_base", "body-v1\n", ""}, {"dry_run", "body-v1\n", ""},
		{"unmanaged_entry", "body-v1\n", "unmanaged"}, {"malformed_marker", "body-v1\n", "malformed"},
		{"invalid_marker_identity", "body-v1\n", "identity"}, {"orphaned", "body-v1\n", "orphaned"},
		{"stale_library_changed", "body-v1\n", "stale"}, {"harness_changed", "body-v1\n", "harness"},
		{"conflict", "body-v1\n", "conflict"}, {"converged_is_in_sync", "body-v1\n", "converged"},
		{"added_and_deleted_drift", "body-v1\n", "added-deleted"},
	}
}
func runCase(def caseDef) expected {
	caseRoot, err := os.MkdirTemp("", "symbrain-skills-install-oracle-")
	if err != nil {
		fatalf("tempdir: %v", err)
	}
	defer os.RemoveAll(caseRoot)
	home, library, base := filepath.Join(caseRoot, "home"), filepath.Join(caseRoot, "library"), filepath.Join(caseRoot, "custom-base")
	name := "oracle-skill"
	source := filepath.Join(library, name)
	writeSkill(source, def.Body)
	result := expected{Statuses: []statusState{}, Artifacts: []fileState{}}
	shouldInstall := def.ID == "fresh_copy_custom_base" || def.ID == "dry_run" || (def.Mutation != "unmanaged" && def.Mutation != "malformed" && def.Mutation != "identity")
	if shouldInstall {
		bundle, err := skill.LoadBundle(source)
		if err != nil {
			fatalf("load %s: %v", def.ID, err)
		}
		renderRoot := filepath.Join(caseRoot, "render")
		_ = os.MkdirAll(renderRoot, 0o700)
		items, errs := render.RenderAll(bundle, renderRoot, []render.Target{render.TargetOpenCode})
		if len(errs) != 0 || len(items) != 1 {
			fatalf("render %s: %v", def.ID, errs)
		}
		opts := install.Options{HomeDir: home, Scope: render.ScopeUser, Mode: install.ModeCopy, BaseDir: base, DryRun: def.ID == "dry_run"}
		installed, err := install.Install(install.RenderedSkill{Target: items[0].Target, Name: name, Path: items[0].Path}, opts)
		if err != nil {
			fatalf("install %s: %v", def.ID, err)
		}
		result.Action, result.Mode = string(installed.Action), installed.Mode
	}
	dest := filepath.Join(home, ".config", "opencode", "skills", name)
	applyMutation(def.Mutation, source, dest, caseRoot)
	statuses, err := install.Status(install.StatusOptions{HomeDir: home, Scope: render.ScopeUser, LibraryDir: library, BaseDir: base, Targets: []render.Target{render.TargetOpenCode}})
	if err != nil {
		fatalf("status %s: %v", def.ID, err)
	}
	for _, st := range statuses {
		installedAt := ""
		if st.InstalledAt != "" {
			installedAt = timestamp
		}
		result.Statuses = append(result.Statuses, statusState{Target: string(st.Target), Name: st.Name, Path: normalize(caseRoot, st.Path), Status: st.Status, Mode: st.Mode, InstalledAt: installedAt, SourceHash: st.SourceHash, Error: normalize(caseRoot, st.Error), Drift: st.Drift})
	}
	result.Artifacts = artifacts(caseRoot, home, base)
	return result
}
func applyMutation(kind, source, dest, caseRoot string) {
	switch kind {
	case "unmanaged":
		os.MkdirAll(dest, 0o755)
		writeFile(filepath.Join(dest, ".symskills.json"), []byte("oracle"), 0o644)
	case "malformed":
		os.MkdirAll(dest, 0o755)
		writeFile(filepath.Join(dest, ".symskills.json"), []byte("{bad"), 0o644)
	case "identity":
		os.MkdirAll(dest, 0o755)
		writeFile(filepath.Join(dest, ".symskills.json"), []byte(`{"schema_version":1,"managed_by":"other","target":"opencode","name":"oracle-skill","mode":"copy"}`), 0o644)
	case "orphaned":
		os.RemoveAll(source)
	case "stale":
		writeSkill(source, "body-v2\n")
	case "harness":
		appendFile(filepath.Join(dest, "SKILL.md"), "harness-edit\n")
	case "conflict":
		writeSkill(source, "body-v2\n")
		appendFile(filepath.Join(dest, "SKILL.md"), "harness-edit\n")
	case "added-deleted":
		os.Remove(filepath.Join(dest, "reference.txt"))
		writeFile(filepath.Join(dest, "harness.txt"), []byte("harness\n"), 0o644)
	case "converged":
		marker, err := os.ReadFile(filepath.Join(dest, ".symskills.json"))
		if err != nil {
			fatalf("converged marker: %v", err)
		}
		writeSkill(source, "body-v2\n")
		bundle, err := skill.LoadBundle(source)
		if err != nil {
			fatalf("converged load: %v", err)
		}
		root := filepath.Join(caseRoot, "converged-render")
		os.MkdirAll(root, 0o700)
		items, errs := render.RenderAll(bundle, root, []render.Target{render.TargetOpenCode})
		if len(errs) != 0 {
			fatalf("converged render: %v", errs)
		}
		os.RemoveAll(dest)
		copyTree(items[0].Path, dest)
		writeFile(filepath.Join(dest, ".symskills.json"), marker, 0o644)
	}
}
func writeSkill(dir, body string) {
	os.MkdirAll(dir, 0o755)
	writeFile(filepath.Join(dir, "SKILL.md"), []byte("---\nname: oracle-skill\ndescription: oracle fixture\n---\n"+body), 0o644)
	writeFile(filepath.Join(dir, "reference.txt"), []byte("reference\n"), 0o755)
}
func appendFile(path, text string) {
	data, err := os.ReadFile(path)
	if err != nil {
		fatalf("read mutation: %v", err)
	}
	writeFile(path, append(data, []byte(text)...), 0o644)
}
func writeFile(path string, data []byte, mode fs.FileMode) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		fatalf("mkdir: %v", err)
	}
	if err := os.WriteFile(path, data, mode); err != nil {
		fatalf("write: %v", err)
	}
	_ = os.Chmod(path, mode)
}
func copyTree(src, dst string) {
	err := filepath.WalkDir(src, func(path string, e fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(src, path)
		target := filepath.Join(dst, rel)
		if rel == "." {
			return os.MkdirAll(dst, 0o755)
		}
		info, err := e.Info()
		if err != nil {
			return err
		}
		if e.IsDir() {
			return os.MkdirAll(target, info.Mode().Perm())
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return writeCopy(target, data, info.Mode().Perm())
	})
	if err != nil {
		fatalf("copy tree: %v", err)
	}
}
func writeCopy(path string, data []byte, mode fs.FileMode) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(path, data, mode); err != nil {
		return err
	}
	return os.Chmod(path, mode)
}
func artifacts(caseRoot string, roots ...string) []fileState {
	var result = []fileState{}
	for _, root := range roots {
		if _, err := os.Lstat(root); err != nil {
			continue
		}
		_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
			if err != nil || path == root {
				return nil
			}
			info, statErr := os.Lstat(path)
			if statErr != nil {
				return nil
			}
			if strings.HasPrefix(filepath.Base(path), ".symskills-lock-") {
				return nil
			}
			relative, relErr := filepath.Rel(caseRoot, path)
			if relErr != nil {
				return nil
			}
			state := fileState{Path: "<root>/" + filepath.ToSlash(relative), Mode: uint32(info.Mode().Perm())}
			switch {
			case info.IsDir():
				state.Type = "dir"
			case info.Mode().IsRegular():
				state.Type = "file"
				state.Bytes, _ = os.ReadFile(path)
				if filepath.Base(path) == ".symskills.json" {
					state.Bytes = normalizeMarkerBytes(state.Bytes)
				}
			default:
				state.Type = "other"
			}
			result = append(result, state)
			return nil
		})
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Path < result[j].Path })
	return result
}

func normalizeMarkerBytes(data []byte) []byte {
	var object map[string]any
	if json.Unmarshal(data, &object) != nil {
		return data
	}
	if _, ok := object["rendered_at"]; ok {
		object["rendered_at"] = "<rendered>"
	}
	if _, ok := object["installed"]; ok {
		object["installed"] = timestamp
	}
	normalized, err := json.MarshalIndent(object, "", "  ")
	if err != nil {
		return data
	}
	return append(normalized, '\n')
}

func normalize(root, value string) string {
	return strings.ReplaceAll(filepath.ToSlash(value), filepath.ToSlash(root), "<root>")
}
func repoRoot() string {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		fatalf("locate oracle")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}
func sourceHashes(root string) map[string]string {
	paths := []string{"internal/skills/install/install.go", "internal/skills/install/status.go", "internal/skills/install/base.go", "internal/skills/install/classify.go", "internal/skills/render/render_target.go"}
	out := map[string]string{}
	for _, p := range paths {
		out[p] = fileSHA256(filepath.Join(root, filepath.FromSlash(p)))
	}
	return out
}
func fileSHA256(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		fatalf("read source %s: %v", path, err)
	}
	sum := sha256.Sum256(data)
	return fmt.Sprintf("%x", sum[:])
}
func fatalf(format string, args ...any) { fmt.Fprintf(os.Stderr, format+"\n", args...); os.Exit(1) }
