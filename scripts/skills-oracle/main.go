// Command skills-oracle freezes the Go skill loader and OpenCode renderer contract.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/variant"
)

type expectedBundle struct {
	ID            string                       `json:"id"`
	Name          string                       `json:"name"`
	Description   string                       `json:"description"`
	Category      string                       `json:"category"`
	Version       string                       `json:"version"`
	Body          string                       `json:"body"`
	Resources     []skill.Resource             `json:"resources"`
	MarkdownPaths []string                     `json:"markdown_paths"`
	OverridePaths map[string][]string          `json:"override_paths"`
	ManifestTerms map[string]map[string]string `json:"manifest_terms"`
	TargetNames   []string                     `json:"target_names"`
}

type variantGolden struct {
	ID     string            `json:"id"`
	Target string            `json:"target"`
	Body   string            `json:"body"`
	Files  map[string]string `json:"files"`
	Issues []string          `json:"issues"`
}

type diagnosticProblem struct {
	Code     string `json:"code"`
	Severity string `json:"severity"`
	Message  string `json:"message"`
	Line     int    `json:"line"`
}

type diagnosticCase struct {
	ID       string              `json:"id"`
	Problems []diagnosticProblem `json:"problems"`
}

type renderedFile struct {
	Path  string `json:"path"`
	Mode  string `json:"mode"`
	Bytes []byte `json:"bytes"`
}

type renderGolden struct {
	ID                string                 `json:"id"`
	Target            string                 `json:"target"`
	Name              string                 `json:"name"`
	Compatibility     string                 `json:"compatibility"`
	SkillMD           []byte                 `json:"skill_md"`
	Files             []renderedFile         `json:"files"`
	Warnings          []string               `json:"warnings"`
	UnmetRequirements []render.CapabilityGap `json:"unmet_requirements"`
	SourceHash        string                 `json:"source_hash"`
}

type securityCase struct {
	ID       string `json:"id"`
	Rejected bool   `json:"rejected"`
	Error    string `json:"error,omitempty"`
}

type hashCase struct {
	Initial        string `json:"initial"`
	AfterContent   string `json:"after_content"`
	AfterMode      string `json:"after_mode"`
	ContentChanged bool   `json:"content_changed"`
	ModeChanged    bool   `json:"mode_changed"`
}

type suite struct {
	Cases         []expectedBundle `json:"cases"`
	VariantCases  []variantGolden  `json:"variant_cases"`
	Diagnostics   []diagnosticCase `json:"diagnostics"`
	RenderCases   []renderGolden   `json:"render_cases"`
	SecurityCases []securityCase   `json:"security_cases"`
	HashCase      hashCase         `json:"hash_case"`
}

func generate() (suite, error) {
	ids := []string{"source", "variant-source", "oracle-edge"}
	result := suite{Cases: make([]expectedBundle, 0, len(ids))}
	for _, id := range ids {
		bundle, err := skill.LoadBundle(filepath.Join("internal", "skills", "render", "testdata", id))
		if err != nil {
			return suite{}, fmt.Errorf("load %s: %w", id, err)
		}
		markdown := make([]string, 0, len(bundle.Markdown))
		for path := range bundle.Markdown {
			markdown = append(markdown, path)
		}
		sort.Strings(markdown)
		overrides := make(map[string][]string, len(bundle.BlockOverrides))
		for target, blocks := range bundle.BlockOverrides {
			ids := make([]string, 0, len(blocks))
			for block := range blocks {
				ids = append(ids, block)
			}
			sort.Strings(ids)
			overrides[target] = ids
		}
		targets := make([]string, 0, len(bundle.Manifest.Targets))
		for target := range bundle.Manifest.Targets {
			targets = append(targets, target)
		}
		sort.Strings(targets)
		terms := bundle.Manifest.Terms
		if terms == nil {
			terms = map[string]map[string]string{}
		}
		result.Cases = append(result.Cases, expectedBundle{
			ID: id, Name: bundle.Frontmatter.Name, Description: bundle.Frontmatter.Description,
			Category: bundle.Frontmatter.Category, Version: bundle.Frontmatter.Version,
			Body: bundle.Body, Resources: bundle.Resources, MarkdownPaths: markdown,
			OverridePaths: overrides, ManifestTerms: terms, TargetNames: targets,
		})
	}
	variantBundle, err := skill.LoadBundle(filepath.Join("internal", "skills", "render", "testdata", "variant-source"))
	if err != nil {
		return suite{}, fmt.Errorf("load variant-source for golden cases: %w", err)
	}
	for _, target := range []string{"antigravity", "claude", "codex", "hermes", "openclaw", "opencode"} {
		opts := variant.Options{Target: target, Overrides: variantBundle.BlockOverrides[target], Terms: variantBundle.Manifest.Terms}
		body, problems := variant.Apply(variantBundle.Body, opts)
		issues := make([]string, 0, len(problems))
		for _, problem := range problems {
			issues = append(issues, problem.String())
		}
		files := map[string]string{}
		for path, source := range variantBundle.Markdown {
			resolved, fileProblems := variant.Apply(source, opts)
			for _, problem := range fileProblems {
				issues = append(issues, problem.String())
			}
			files[path] = resolved.Text
		}
		result.VariantCases = append(result.VariantCases, variantGolden{ID: target, Target: target, Body: body.Text, Files: files, Issues: issues})
	}
	known := []string{"claude", "hermes"}
	toDiagnostics := func(id string, problems []variant.Problem) diagnosticCase {
		items := make([]diagnosticProblem, 0, len(problems))
		for _, problem := range problems {
			items = append(items, diagnosticProblem{Code: problem.Code, Severity: problem.Severity, Message: problem.Message, Line: problem.Line})
		}
		return diagnosticCase{ID: id, Problems: items}
	}
	result.Diagnostics = []diagnosticCase{
		toDiagnostics("unknown_region_target", variant.CheckRegionTargets([]variant.Region{{Kind: variant.KindOnly, Targets: []string{"hermez"}, Line: 4}}, known)),
		toDiagnostics("unknown_override", variant.CheckOverrides([]string{"worker"}, map[string][]string{"claude": {"invented"}})),
		toDiagnostics("term_without_default", variant.CheckTerms(map[string]map[string]string{"report_dir": {"hermes": "value"}}, known)),
		toDiagnostics("term_unknown_target", variant.CheckTerms(map[string]map[string]string{"report_dir": {variant.DefaultKey: "value", "hermez": "value"}}, known)),
	}
	_, problems := variant.Apply("{{term:Bad Name}}\n", variant.Options{Target: "hermes"})
	result.Diagnostics = append(result.Diagnostics, toDiagnostics("invalid_term_reference", problems))
	_, problems = variant.Apply("{{term:missing}}\n", variant.Options{Target: "hermes"})
	result.Diagnostics = append(result.Diagnostics, toDiagnostics("unknown_term_reference", problems))
	_, problems = variant.Apply("{{term:no_default}}\n", variant.Options{Target: "hermes", Terms: map[string]map[string]string{"no_default": {"claude": "value"}}})
	result.Diagnostics = append(result.Diagnostics, toDiagnostics("missing_term_default", problems))

	for _, id := range []string{"source", "variant-source", "oracle-edge"} {
		bundle, err := skill.LoadBundle(filepath.Join("internal", "skills", "render", "testdata", id))
		if err != nil {
			return suite{}, err
		}
		golden, err := renderAndManifest(bundle, id+"/opencode")
		if err != nil {
			return suite{}, err
		}
		result.RenderCases = append(result.RenderCases, golden)
	}
	result.SecurityCases, err = securityCases()
	if err != nil {
		return suite{}, err
	}
	result.HashCase, err = hashInvalidationCase()
	if err != nil {
		return suite{}, err
	}
	return result, nil
}

func renderAndManifest(bundle *skill.Bundle, id string) (renderGolden, error) {
	out, err := os.MkdirTemp("", "skills-oracle-render-")
	if err != nil {
		return renderGolden{}, err
	}
	defer os.RemoveAll(out)
	items, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) != 0 {
		return renderGolden{}, errs[0]
	}
	if len(items) != 1 {
		return renderGolden{}, fmt.Errorf("render %s: want one item, got %d", id, len(items))
	}
	item := items[0]
	files, err := manifest(item.Path)
	if err != nil {
		return renderGolden{}, err
	}
	marker, err := os.ReadFile(filepath.Join(item.Path, ".symskills.json"))
	if err != nil {
		return renderGolden{}, err
	}
	var markerObject struct {
		SourceHash string `json:"source_hash"`
	}
	if err := json.Unmarshal(marker, &markerObject); err != nil {
		return renderGolden{}, err
	}
	return renderGolden{ID: id, Target: string(item.Target), Name: item.Name, Compatibility: item.Frontmatter.Compatibility, SkillMD: []byte(item.SkillMD), Files: files, Warnings: item.Warnings, UnmetRequirements: item.UnmetRequirements, SourceHash: markerObject.SourceHash}, nil
}

func manifest(root string) ([]renderedFile, error) {
	var files []renderedFile
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return nil
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		files = append(files, renderedFile{Path: filepath.ToSlash(rel), Mode: fmt.Sprintf("%04o", info.Mode().Perm()), Bytes: data})
		return nil
	})
	sort.Slice(files, func(i, j int) bool { return files[i].Path < files[j].Path })
	return files, err
}

func securityCases() ([]securityCase, error) {
	result, err := renderValidationCases()
	if err != nil {
		return nil, err
	}
	unknownRoot, err := os.MkdirTemp("", "skills-oracle-unknown-target-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(unknownRoot)
	writeSkillFixture(unknownRoot, "security-skill", "")
	unknownBundle, err := skill.LoadBundle(unknownRoot)
	if err != nil {
		return nil, err
	}
	_, unknownErr := render.RenderTarget(unknownBundle, render.Target("not-a-target"))
	result = append(result, securityCase{ID: "unknown_target", Rejected: unknownErr != nil, Error: errorText(unknownErr)})
	cases := []struct{ id, reference string }{{"absolute_overlay", "/etc/passwd"}, {"parent_overlay", "../escape.md"}}
	for _, item := range cases {
		root, err := os.MkdirTemp("", "skills-oracle-security-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(root)
		writeSkillFixture(root, "security-skill", fmt.Sprintf("[targets.opencode]\nenabled = true\nprepend = %q\n", item.reference))
		bundle, err := skill.LoadBundle(root)
		if err != nil {
			return nil, err
		}
		_, renderErr := render.RenderTarget(bundle, render.TargetOpenCode)
		result = append(result, securityCase{ID: item.id, Rejected: renderErr != nil, Error: errorText(renderErr)})
	}
	root, err := os.MkdirTemp("", "skills-oracle-security-")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(root)
	writeSkillFixture(root, "security-skill", "[targets.opencode]\nenabled = true\nalias = \"../escape\"\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		return nil, err
	}
	_, renderErr := render.RenderTarget(bundle, render.TargetOpenCode)
	result = append(result, securityCase{ID: "resolved_name_traversal", Rejected: renderErr != nil, Error: errorText(renderErr)})
	for _, item := range []struct {
		id    string
		final bool
	}{
		{id: "destination_parent_symlink"},
		{id: "destination_final_symlink", final: true},
	} {
		root, err := os.MkdirTemp("", "skills-oracle-destination-skill-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(root)
		writeSkillFixture(root, "security-skill", "")
		bundle, err := skill.LoadBundle(root)
		if err != nil {
			return nil, err
		}
		out, err := os.MkdirTemp("", "skills-oracle-destination-output-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(out)
		outside, err := os.MkdirTemp("", "skills-oracle-destination-outside-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(outside)
		link := filepath.Join(out, "opencode")
		if item.final {
			if err := os.MkdirAll(link, 0o755); err != nil {
				return nil, err
			}
			link = filepath.Join(link, "security-skill")
		}
		if err := os.Symlink(outside, link); err != nil {
			return nil, err
		}
		_, renderErrs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
		var renderErr error
		if len(renderErrs) > 0 {
			renderErr = renderErrs[0]
		}
		result = append(result, securityCase{ID: item.id, Rejected: renderErr != nil, Error: errorText(renderErr)})
	}
	return result, nil
}

func renderValidationCases() ([]securityCase, error) {
	cases := []struct {
		id       string
		manifest string
		skill    string
		files    map[string]string
	}{
		{
			id:       "missing_configured_overlay",
			manifest: "[targets.opencode]\nenabled = true\nprepend = \"missing.md\"\n",
		},
		{
			id: "orphan_override",
			files: map[string]string{
				"overlays/opencode/blocks/invented.md": "replacement\n",
			},
		},
		{
			id:    "duplicate_block_id_across_files",
			skill: "---\nname: security-skill\ndescription: security fixture\n---\n<!-- symskills:block shared -->\nOne.\n<!-- /symskills:block -->\n",
			files: map[string]string{
				"references/other.md": "<!-- symskills:block shared -->\nTwo.\n<!-- /symskills:block -->\n",
			},
		},
	}
	result := make([]securityCase, 0, len(cases))
	for _, item := range cases {
		root, err := os.MkdirTemp("", "skills-oracle-validation-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(root)
		writeSkillFixture(root, "security-skill", item.manifest)
		if item.skill != "" {
			if err := os.WriteFile(filepath.Join(root, "SKILL.md"), []byte(item.skill), 0o644); err != nil {
				return nil, err
			}
		}
		for path, content := range item.files {
			fullPath := filepath.Join(root, filepath.FromSlash(path))
			if err := os.MkdirAll(filepath.Dir(fullPath), 0o755); err != nil {
				return nil, err
			}
			if err := os.WriteFile(fullPath, []byte(content), 0o644); err != nil {
				return nil, err
			}
		}
		bundle, err := skill.LoadBundle(root)
		if err != nil {
			return nil, err
		}
		_, renderErr := render.RenderTarget(bundle, render.TargetOpenCode)
		result = append(result, securityCase{ID: item.id, Rejected: renderErr != nil, Error: errorText(renderErr)})
	}
	return result, nil
}

func hashInvalidationCase() (hashCase, error) {
	root, err := os.MkdirTemp("", "skills-oracle-hash-")
	if err != nil {
		return hashCase{}, err
	}
	defer os.RemoveAll(root)
	writeSkillFixture(root, "hash-skill", "")
	if err := os.MkdirAll(filepath.Join(root, "scripts"), 0o755); err != nil {
		return hashCase{}, err
	}
	script := filepath.Join(root, "scripts", "tool.sh")
	if err := os.WriteFile(script, []byte("v1\n"), 0o644); err != nil {
		return hashCase{}, err
	}
	out, err := os.MkdirTemp("", "skills-oracle-hash-output-")
	if err != nil {
		return hashCase{}, err
	}
	defer os.RemoveAll(out)
	renderHash := func() (string, error) {
		bundle, err := skill.LoadBundle(root)
		if err != nil {
			return "", err
		}
		items, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
		if len(errs) != 0 {
			return "", errs[0]
		}
		data, err := os.ReadFile(filepath.Join(items[0].Path, ".symskills.json"))
		if err != nil {
			return "", err
		}
		var marker struct {
			SourceHash string `json:"source_hash"`
		}
		if err := json.Unmarshal(data, &marker); err != nil {
			return "", err
		}
		return marker.SourceHash, nil
	}
	initial, err := renderHash()
	if err != nil {
		return hashCase{}, err
	}
	if err := os.WriteFile(script, []byte("v2\n"), 0o644); err != nil {
		return hashCase{}, err
	}
	afterContent, err := renderHash()
	if err != nil {
		return hashCase{}, err
	}
	if err := os.Chmod(script, 0o755); err != nil && runtime.GOOS != "windows" {
		return hashCase{}, err
	}
	afterMode, err := renderHash()
	if err != nil {
		return hashCase{}, err
	}
	return hashCase{Initial: initial, AfterContent: afterContent, AfterMode: afterMode, ContentChanged: initial != afterContent, ModeChanged: afterContent != afterMode}, nil
}

func writeSkillFixture(root, name, manifest string) {
	_ = os.WriteFile(filepath.Join(root, "SKILL.md"), []byte("---\nname: "+name+"\ndescription: security fixture\n---\nBody.\n"), 0o644)
	_ = os.WriteFile(filepath.Join(root, "symskills.toml"), []byte("[skill]\nname = \""+name+"\"\n"+manifest), 0o644)
}
func errorText(err error) string {
	if err == nil {
		return ""
	}
	return strings.TrimSpace(err.Error())
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-skills/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()
	generated, err := generate()
	if err != nil {
		fmt.Fprintf(os.Stderr, "generate skills oracle: %v\n", err)
		os.Exit(1)
	}
	data, err := json.MarshalIndent(generated, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "marshal skills oracle: %v\n", err)
		os.Exit(1)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", *output, err)
			os.Exit(1)
		}
		if !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/skills-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: skills oracle deterministic check passed")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir %s: %v\n", filepath.Dir(*output), err)
		os.Exit(1)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "write %s: %v\n", *output, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s\n", *output)
}
