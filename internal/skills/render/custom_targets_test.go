package render

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// customTargetsSnapshot returns the length of the global registry so tests
// can assert on the delta without depending on the built-in count.
func customTargetsSnapshot() int {
	return len(Targets)
}

func TestRegisterCustomTargetsAppendsToRegistry(t *testing.T) {
	before := customTargetsSnapshot()
	defer func() { Targets = Targets[:before] }()

	err := RegisterCustomTargets([]CustomTargetSpec{
		{Name: "myagent", SkillRootUser: "/home/u/.myagent/skills", SkillRootProject: "/proj/.myagent/skills"},
	})
	if err != nil {
		t.Fatalf("register: %v", err)
	}
	if len(Targets) != before+1 {
		t.Fatalf("expected %d targets, got %d", before+1, len(Targets))
	}
	spec, ok := LookupSpec("myagent")
	if !ok {
		t.Fatal("custom target not found in registry")
	}
	if got := spec.SkillRoot("/home/u", "/proj", ScopeUser); got != "/home/u/.myagent/skills" {
		t.Errorf("user skill root = %q", got)
	}
	if got := spec.SkillRoot("/home/u", "/proj", ScopeProject); got != "/proj/.myagent/skills" {
		t.Errorf("project skill root = %q", got)
	}
	if got := spec.ConfigDir("/home/u", "/proj", ScopeUser); got != "/home/u/.myagent" {
		t.Errorf("config dir = %q", got)
	}
	if got := spec.ConfigDir("/home/u", "/proj", ScopeProject); got != "/proj/.myagent" {
		t.Errorf("project config dir = %q", got)
	}
	// ParseTarget must accept the custom name.
	if _, err := ParseTarget("myagent"); err != nil {
		t.Errorf("ParseTarget(custom): %v", err)
	}
}

func TestRegisterCustomTargetsCollision(t *testing.T) {
	before := customTargetsSnapshot()
	defer func() { Targets = Targets[:before] }()

	err := RegisterCustomTargets([]CustomTargetSpec{
		{Name: "opencode", SkillRootUser: "/tmp/x"},
	})
	if err == nil {
		t.Fatal("expected collision error for built-in name opencode")
	}
	if len(Targets) != before {
		t.Errorf("registry mutated on collision: %d -> %d", before, len(Targets))
	}

	// Duplicate custom names must also collide.
	if err := RegisterCustomTargets([]CustomTargetSpec{{Name: "myagent", SkillRootUser: "/tmp/a"}}); err != nil {
		t.Fatalf("first register: %v", err)
	}
	if err := RegisterCustomTargets([]CustomTargetSpec{{Name: "myagent", SkillRootUser: "/tmp/b"}}); err == nil {
		t.Fatal("expected collision error for duplicate custom name")
	}
}

func TestRegisterCustomTargetsValidation(t *testing.T) {
	before := customTargetsSnapshot()
	defer func() { Targets = Targets[:before] }()

	if err := RegisterCustomTargets([]CustomTargetSpec{{Name: "", SkillRootUser: "/tmp/x"}}); err == nil {
		t.Fatal("expected error for empty name")
	}
	if err := RegisterCustomTargets([]CustomTargetSpec{{Name: "x", SkillRootUser: ""}}); err == nil {
		t.Fatal("expected error for empty skill_root_user")
	}
}

func TestCustomTargetOverlayDir(t *testing.T) {
	before := customTargetsSnapshot()
	defer func() { Targets = Targets[:before] }()

	if err := RegisterCustomTargets([]CustomTargetSpec{
		{Name: "custom", SkillRootUser: "/tmp/custom-root", OverlayDir: "my-overlays"},
	}); err != nil {
		t.Fatalf("register: %v", err)
	}
	if got := overlayDir("custom"); got != "my-overlays" {
		t.Errorf("overlayDir = %q, want my-overlays", got)
	}
	if got := overlayDir("opencode"); got != "opencode" {
		t.Errorf("built-in overlayDir = %q, want opencode", got)
	}
}

func TestDefaultTargetsMirrorsRegistry(t *testing.T) {
	got := DefaultTargets()
	if len(got) != len(Targets) {
		t.Fatalf("DefaultTargets returned %d targets, registry has %d", len(got), len(Targets))
	}
	for i, target := range got {
		if target != Targets[i].Name {
			t.Errorf("DefaultTargets()[%d] = %q, want %q", i, target, Targets[i].Name)
		}
	}
}

func TestRenderCustomTargetMetadata(t *testing.T) {
	before := customTargetsSnapshot()
	defer func() { Targets = Targets[:before] }()

	template := filepath.Join(t.TempDir(), "metadata.txt")
	if err := os.WriteFile(template, []byte("custom metadata\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := RegisterCustomTargets([]CustomTargetSpec{
		{
			Name:             "metadata-target",
			SkillRootUser:    "/tmp/metadata-target/skills",
			MetadataFile:     "meta/config.txt",
			MetadataTemplate: template,
		},
	}); err != nil {
		t.Fatalf("register: %v", err)
	}

	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: metadata-skill\ndescription: test\n---\n# Body\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, out, []Target{"metadata-target"})
	if len(errs) != 0 {
		t.Fatalf("RenderAll errors: %v", errs)
	}
	if len(results) != 1 {
		t.Fatalf("got %d render results, want 1", len(results))
	}
	metadataPath := filepath.Join(out, "metadata-target", "metadata-skill", "meta", "config.txt")
	data, err := os.ReadFile(metadataPath)
	if err != nil {
		t.Fatalf("read metadata: %v", err)
	}
	if string(data) != "custom metadata\n" {
		t.Fatalf("metadata = %q", data)
	}
}

func TestRenderCustomTargetMetadataErrors(t *testing.T) {
	for _, tc := range []struct {
		name     string
		target   string
		template string
		want     string
	}{
		{name: "missing template declaration", target: "metadata-no-template", want: "requires metadata_template"},
		{name: "missing template file", target: "metadata-missing-file", template: filepath.Join(t.TempDir(), "missing"), want: "read metadata template"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			before := customTargetsSnapshot()
			defer func() { Targets = Targets[:before] }()

			if err := RegisterCustomTargets([]CustomTargetSpec{
				{Name: tc.target, SkillRootUser: "/tmp/metadata-target/skills", MetadataFile: "meta/config.txt", MetadataTemplate: tc.template},
			}); err != nil {
				t.Fatalf("register: %v", err)
			}

			root := t.TempDir()
			writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: metadata-skill\ndescription: test\n---\n# Body\n")
			bundle, err := skill.LoadBundle(root)
			if err != nil {
				t.Fatal(err)
			}

			results, errs := RenderAll(bundle, filepath.Join(t.TempDir(), "rendered"), []Target{Target(tc.target)})
			if len(results) != 0 || len(errs) != 1 {
				t.Fatalf("RenderAll results=%d errors=%d, want 0 results and 1 error: %v", len(results), len(errs), errs)
			}
			if !strings.Contains(errs[0].Error(), tc.want) {
				t.Fatalf("error %q does not contain %q", errs[0], tc.want)
			}
		})
	}
}
