package install

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
)

// writeSkill creates a minimal portable skill directory and returns its root.
func writeSkill(t *testing.T, root, name, body string) string {
	t.Helper()
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "SKILL.md"), []byte("---\nname: "+name+"\ndescription: test\n---\n\n"+body+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	return root
}

// installAndStage runs the standard pull pipeline: render, install, mutate the
// installed tree, pull (staged). Returns the pull options and result.
func installAndStage(t *testing.T, name string) (PullOptions, PullResult) {
	t.Helper()
	home, lib := t.TempDir(), t.TempDir()
	src := filepath.Join(lib, name)
	if err := os.MkdirAll(filepath.Join(src, "overlays", "opencode"), 0o755); err != nil {
		t.Fatal(err)
	}
	writeSkill(t, src, name, "portable v1")
	if err := os.WriteFile(filepath.Join(src, "overlays", "opencode", "prepend.md"), []byte("overlay header\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "notes.txt"), []byte("v1\n"), 0o751); err != nil {
		t.Fatal(err)
	}
	bundle, err := skill.LoadBundle(src)
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) > 0 {
		t.Fatal(errs[0])
	}
	opts := PullOptions{HomeDir: home, Scope: render.ScopeUser, LibraryDir: lib, BaseDir: filepath.Join(home, ".base"), Target: render.TargetOpenCode, Name: name}
	if _, err := Install(RenderedSkill{Target: render.TargetOpenCode, Name: name, Path: rendered[0].Path}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, BaseDir: filepath.Join(home, ".base")}); err != nil {
		t.Fatal(err)
	}
	dest, err := InstallPath(render.TargetOpenCode, name, Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(dest, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	data = []byte(strings.Replace(string(data), "portable v1", "portable v2", 1))
	if err := os.WriteFile(filepath.Join(dest, "SKILL.md"), data, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dest, "added.sh"), []byte("#!/bin/sh\n"), 0o751); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(dest, "notes.txt")); err != nil {
		t.Fatal(err)
	}
	result, err := Pull(opts)
	if err != nil {
		t.Fatal(err)
	}
	if result.Action != "staged" || result.StagePath == "" {
		t.Fatalf("result=%+v", result)
	}
	return opts, result
}

func TestApplyPendingPromotesStageIntoLibrary(t *testing.T) {
	opts, result := installAndStage(t, "promote")
	if err := ApplyPending(opts); err != nil {
		t.Fatal(err)
	}
	libSKILL, err := os.ReadFile(filepath.Join(opts.LibraryDir, "promote", "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(libSKILL), "portable v2") {
		t.Fatalf("library SKILL.md not updated: %q", libSKILL)
	}
	if info, err := os.Stat(filepath.Join(opts.LibraryDir, "promote", "added.sh")); err != nil || info.Mode().Perm() != 0o751 {
		t.Fatalf("added.sh missing or wrong mode: %v %v", info, err)
	}
	if _, err := os.Stat(filepath.Join(opts.LibraryDir, "promote", "notes.txt")); !os.IsNotExist(err) {
		t.Fatalf("notes.txt should be gone, got %v", err)
	}
	if _, err := os.Stat(result.StagePath); !os.IsNotExist(err) {
		t.Fatalf("stage %s should be removed after apply, got %v", result.StagePath, err)
	}
	if _, err := os.Stat(filepath.Join(opts.LibraryDir, "promote", pullManifestFile)); !os.IsNotExist(err) {
		t.Fatalf("manifest must not leak into library: %v", err)
	}
}

func TestApplyPendingRefusesWithoutPendingManifest(t *testing.T) {
	err := ApplyPending(PullOptions{
		HomeDir:    t.TempDir(),
		LibraryDir: t.TempDir(),
		PendingDir: t.TempDir(),
		Target:     render.TargetOpenCode,
		Name:       "nopending",
	})
	if err == nil || !strings.Contains(err.Error(), "no pending pull") {
		t.Fatalf("expected no-pending error, got %v", err)
	}
}

func TestApplyPendingRequiresLibraryDir(t *testing.T) {
	pending := t.TempDir()
	stage := filepath.Join(pending, "opencode", "skillx")
	if err := os.MkdirAll(stage, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stage, pullManifestFile), []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), PendingDir: pending, Target: render.TargetOpenCode, Name: "skillx"})
	if err == nil || !strings.Contains(err.Error(), "apply requires library directory") {
		t.Fatalf("expected library-dir error, got %v", err)
	}
}

func TestApplyPendingFailsWhenLibraryTargetMissing(t *testing.T) {
	pending := t.TempDir()
	stage := filepath.Join(pending, "opencode", "skillx")
	if err := os.MkdirAll(stage, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stage, pullManifestFile), []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	lib := t.TempDir() // empty: no "skillx" directory exists
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), LibraryDir: lib, PendingDir: pending, Target: render.TargetOpenCode, Name: "skillx"})
	if err == nil {
		t.Fatal("expected rename error for missing library target")
	}
}

// loadBundleWithManifest creates a skill with the given symskills.toml content
// and returns the loaded bundle.
func loadBundleWithManifest(t *testing.T, name, manifest string) *skill.Bundle {
	t.Helper()
	root := writeSkill(t, t.TempDir(), name, "portable")
	if manifest != "" {
		if err := os.WriteFile(filepath.Join(root, "symskills.toml"), []byte(manifest), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	return bundle
}

// writeFrontmatterToml writes overlays/<target>/frontmatter.toml into the
// bundle root and returns a freshly loaded bundle.
func writeFrontmatterToml(t *testing.T, bundle *skill.Bundle, content string) *skill.Bundle {
	t.Helper()
	dir := filepath.Join(bundle.Root, "overlays", "opencode")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "frontmatter.toml"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	reloaded, err := skill.LoadBundle(bundle.Root)
	if err != nil {
		t.Fatal(err)
	}
	return reloaded
}

func TestOverlayFrontmatterKeysOwnershipBasics(t *testing.T) {
	bundle := loadBundleWithManifest(t, "fm", `
[skill]
name = "fm"

[targets.opencode]
alias = "fm-open"
description = "OpenCode variant"
metadata = { owner = "core" }
`)
	owned, values, err := overlayFrontmatterKeys(bundle, render.TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	if !owned["compatibility"] {
		t.Error("compatibility must always be owned")
	}
	if !owned["name"] || !owned["description"] || !owned["metadata.owner"] {
		t.Errorf("expected owned name/description/metadata.owner, got %v", owned)
	}
	if values["name"] != "fm-open" || values["description"] != "OpenCode variant" || values["metadata.owner"] != "core" {
		t.Errorf("unexpected overlay values: %v", values)
	}
	if values["compatibility"] != string(render.TargetOpenCode) {
		t.Errorf("compatibility value = %v", values["compatibility"])
	}
}

func TestOverlayFrontmatterKeysReadsFrontmatterToml(t *testing.T) {
	bundle := loadBundleWithManifest(t, "fmt", "")
	bundle = writeFrontmatterToml(t, bundle, "allowed-tools = [\"git\"]\n\n[metadata]\nowner = \"team\"\n")
	owned, values, err := overlayFrontmatterKeys(bundle, render.TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	if !owned["allowed-tools"] || !owned["metadata.owner"] {
		t.Errorf("expected owned allowed-tools and metadata.owner, got %v", owned)
	}
	if values["metadata.owner"] != "team" {
		t.Errorf("metadata.owner value = %v", values["metadata.owner"])
	}
}

func TestOverlayFrontmatterKeysMalformedToml(t *testing.T) {
	bundle := loadBundleWithManifest(t, "fmtbad", "")
	bundle = writeFrontmatterToml(t, bundle, "not [ valid toml\n")
	if _, _, err := overlayFrontmatterKeys(bundle, render.TargetOpenCode); err == nil {
		t.Fatal("expected TOML decode error")
	}
}

func TestOverlayFrontmatterKeysNoTomlUsesDefaults(t *testing.T) {
	bundle := loadBundleWithManifest(t, "fmdefaults", "")
	owned, values, err := overlayFrontmatterKeys(bundle, render.TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	if len(owned) != 1 || !owned["compatibility"] {
		t.Errorf("expected only compatibility owned, got %v", owned)
	}
	if values["compatibility"] != string(render.TargetOpenCode) {
		t.Errorf("compatibility value = %v", values["compatibility"])
	}
}

// newPullResult returns a PullResult with non-nil change/refusal slices, as
// Pull() initializes them.
func newPullResult() *PullResult {
	return &PullResult{Changes: []PullChange{}, FrontmatterChanges: []PullFrontmatterChange{}, Refusals: []string{}}
}

func TestPullFrontmatterRefusesOwnedMetadataChange(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfr", "")
	bundle = writeFrontmatterToml(t, bundle, "[metadata]\nowner = \"team\"\n")
	source := map[string]any{"name": "pfr", "metadata": map[string]any{"owner": "team"}}
	installed := map[string]any{"name": "pfr", "metadata": map[string]any{"owner": "someone-else"}}
	result := newPullResult()
	err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result)
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, "; "), "metadata.owner") {
		t.Fatalf("expected owned metadata refusal, err=%v refusals=%v", err, result.Refusals)
	}
}

func TestPullFrontmatterOwnedMetadataUnchangedIsNotRefused(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfok", "")
	bundle = writeFrontmatterToml(t, bundle, "[metadata]\nowner = \"team\"\n")
	source := map[string]any{"name": "pfok", "metadata": map[string]any{"owner": "team"}}
	installed := map[string]any{"name": "pfok", "metadata": map[string]any{"owner": "team"}}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatalf("unexpected refusal: %v", err)
	}
	if len(result.Refusals) != 0 || len(result.FrontmatterChanges) != 0 {
		t.Fatalf("expected no refusals/changes, got %+v", result)
	}
}

func TestPullFrontmatterPullsNonOwnedMetadataChange(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfm", "")
	source := map[string]any{"name": "pfm", "metadata": map[string]any{"owner": "team"}}
	installed := map[string]any{"name": "pfm", "metadata": map[string]any{"owner": "new-team"}}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatal(err)
	}
	meta := source["metadata"].(map[string]any)
	if meta["owner"] != "new-team" {
		t.Fatalf("source metadata not pulled: %v", source)
	}
	if len(result.FrontmatterChanges) != 1 || result.FrontmatterChanges[0].Key != "metadata.owner" || result.FrontmatterChanges[0].Reason != "frontmatter" {
		t.Fatalf("unexpected changes: %+v", result.FrontmatterChanges)
	}
}

func TestPullFrontmatterDeletesRemovedMetadataKey(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfdel", "")
	source := map[string]any{"name": "pfdel", "metadata": map[string]any{"owner": "team", "kept": "yes"}}
	installed := map[string]any{"name": "pfdel", "metadata": map[string]any{"kept": "yes"}}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatal(err)
	}
	if _, ok := source["metadata"].(map[string]any)["owner"]; ok {
		t.Fatalf("deleted metadata key still present: %v", source)
	}
	found := false
	for _, c := range result.FrontmatterChanges {
		if c.Key == "metadata.owner" && c.To == nil {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected deletion change for metadata.owner: %+v", result.FrontmatterChanges)
	}
}

func TestPullFrontmatterPullsTopLevelChangeWithPermissionReason(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pftl", "")
	source := map[string]any{"name": "pftl", "allowed-tools": "a"}
	installed := map[string]any{"name": "pftl", "allowed-tools": "b"}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatal(err)
	}
	if source["allowed-tools"] != "b" {
		t.Fatalf("top-level key not pulled: %v", source)
	}
	if len(result.FrontmatterChanges) != 1 || result.FrontmatterChanges[0].Key != "allowed-tools" || result.FrontmatterChanges[0].Reason != "permission-relevant frontmatter" {
		t.Fatalf("expected permission-relevant change, got %+v", result.FrontmatterChanges)
	}
}

func TestPullFrontmatterRefusesOwnedTopLevelChange(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfo", "")
	bundle = writeFrontmatterToml(t, bundle, "allowed-tools = [\"git\"]\n")
	source := map[string]any{"name": "pfo", "allowed-tools": "git"}
	installed := map[string]any{"name": "pfo", "allowed-tools": "other"}
	result := newPullResult()
	err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result)
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, "; "), "allowed-tools") {
		t.Fatalf("expected owned top-level refusal, err=%v refusals=%v", err, result.Refusals)
	}
}

func TestPullFrontmatterRefusesChangedCompatibility(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfcomp", "")
	source := map[string]any{"name": "pfcomp", "compatibility": "opencode"}
	installed := map[string]any{"name": "pfcomp", "compatibility": "claude"}
	result := newPullResult()
	err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result)
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, "; "), "compatibility") {
		t.Fatalf("expected compatibility refusal, err=%v refusals=%v", err, result.Refusals)
	}
}

func TestPullFrontmatterSortsChanges(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfsort", "")
	source := map[string]any{"name": "pfsort", "zzz": "1", "aaa": "1", "metadata": map[string]any{"b": "1", "a": "1"}}
	installed := map[string]any{"name": "pfsort", "zzz": "2", "aaa": "2", "metadata": map[string]any{"b": "2", "a": "2"}}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatal(err)
	}
	if len(result.FrontmatterChanges) != 4 {
		t.Fatalf("expected 4 changes, got %+v", result.FrontmatterChanges)
	}
	for i := 1; i < len(result.FrontmatterChanges); i++ {
		if result.FrontmatterChanges[i-1].Key >= result.FrontmatterChanges[i].Key {
			t.Fatalf("changes not sorted: %+v", result.FrontmatterChanges)
		}
	}
}

// bundleWithOverlayTexts creates a skill with prepend.md/append.md overlays.
func bundleWithOverlayTexts(t *testing.T, name, prepend, appendText string) *skill.Bundle {
	t.Helper()
	bundle := loadBundleWithManifest(t, name, "")
	dir := filepath.Join(bundle.Root, "overlays", "opencode")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if prepend != "" {
		if err := os.WriteFile(filepath.Join(dir, "prepend.md"), []byte(prepend), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if appendText != "" {
		if err := os.WriteFile(filepath.Join(dir, "append.md"), []byte(appendText), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	reloaded, err := skill.LoadBundle(bundle.Root)
	if err != nil {
		t.Fatal(err)
	}
	return reloaded
}

func TestPullBodyRefusesPrependRegionChange(t *testing.T) {
	bundle := bundleWithOverlayTexts(t, "pbpre", "header\n", "")
	result := newPullResult()
	out := ""
	err := pullBody("body only\n", "body only\n", bundle, render.TargetOpenCode, result, &out)
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, "; "), "prepend") {
		t.Fatalf("expected prepend refusal, err=%v refusals=%v", err, result.Refusals)
	}
}

func TestPullBodyRefusesAppendRegionChange(t *testing.T) {
	bundle := bundleWithOverlayTexts(t, "pbapp", "", "footer\n")
	result := newPullResult()
	out := ""
	err := pullBody("body only\n", "body only\n", bundle, render.TargetOpenCode, result, &out)
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, "; "), "append") {
		t.Fatalf("expected append refusal, err=%v refusals=%v", err, result.Refusals)
	}
}

func TestPullBodyMergesCleanRegions(t *testing.T) {
	bundle := bundleWithOverlayTexts(t, "pbok", "header\n", "footer\n")
	result := newPullResult()
	out := ""
	installed := "header\n\nmiddle\n\nfooter\n"
	err := pullBody("middle\n", installed, bundle, render.TargetOpenCode, result, &out)
	if err != nil {
		t.Fatal(err)
	}
	if out != "middle\n" {
		t.Fatalf("merged body = %q", out)
	}
	if len(result.Refusals) != 0 {
		t.Fatalf("unexpected refusals: %v", result.Refusals)
	}
	if len(result.Changes) != 0 {
		t.Fatalf("identical body must not be a change: %+v", result.Changes)
	}
}

func TestPullBodyRecordsChangeWhenSourceDiffers(t *testing.T) {
	bundle := bundleWithOverlayTexts(t, "pbch", "header\n", "footer\n")
	result := newPullResult()
	out := ""
	installed := "header\n\nmiddle v2\n\nfooter\n"
	err := pullBody("middle v1\n", installed, bundle, render.TargetOpenCode, result, &out)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Changes) != 1 || result.Changes[0].Path != "SKILL.md" || result.Changes[0].Status != "modified" {
		t.Fatalf("expected SKILL.md change, got %+v", result.Changes)
	}
	if out != "middle v2\n" {
		t.Fatalf("merged body = %q", out)
	}
}

func TestOverlayTextForEscapesRoot(t *testing.T) {
	bundle := loadBundleWithManifest(t, "otfe", "")
	if _, err := overlayTextFor(bundle.Root, render.TargetOpenCode, "prepend.md", "../evil.md"); err == nil {
		t.Fatal("expected escape refusal")
	}
	if _, err := overlayTextFor(bundle.Root, render.TargetOpenCode, "prepend.md", "/absolute/path.md"); err == nil {
		t.Fatal("expected absolute-path refusal")
	}
	if got, err := overlayTextFor(bundle.Root, render.TargetOpenCode, "prepend.md", "overlays/opencode/missing.md"); err != nil || got != "" {
		t.Fatalf("missing overlay must yield empty string, got %q err=%v", got, err)
	}
}

func TestMapValueVariants(t *testing.T) {
	if got := mapValue(map[string]any{"a": "b"}); got["a"] != "b" {
		t.Fatalf("map[string]any: %v", got)
	}
	if got := mapValue(map[string]string{"c": "d"}); got["c"] != "d" {
		t.Fatalf("map[string]string: %v", got)
	}
	if got := mapValue("not a map"); len(got) != 0 {
		t.Fatalf("non-map: %v", got)
	}
	if got := mapValue(nil); len(got) != 0 {
		t.Fatalf("nil: %v", got)
	}
	if got := mapValue(42); len(got) != 0 {
		t.Fatalf("int: %v", got)
	}
}

func TestBodyFromBundle(t *testing.T) {
	if got := bodyFromBundle(&skill.Bundle{Body: "\n\nhello\n"}); got != "hello\n" {
		t.Fatalf("body = %q", got)
	}
	if got := bodyFromBundle(&skill.Bundle{Body: "plain"}); got != "plain" {
		t.Fatalf("body = %q", got)
	}
}

func TestApplyPendingLockHeld(t *testing.T) {
	pending := t.TempDir()
	lockDir := filepath.Join(pending, ".locks", "opencode")
	if err := os.MkdirAll(lockDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// A foreign lockfile (O_EXCL semantics) makes AcquirePullLock fail.
	if err := os.WriteFile(filepath.Join(lockDir, "skillx.lock"), []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), LibraryDir: t.TempDir(), PendingDir: pending, Target: render.TargetOpenCode, Name: "skillx"})
	if err == nil || !strings.Contains(err.Error(), "pull lock held") {
		t.Fatalf("expected lock error, got %v", err)
	}
}

func TestApplyPendingInvalidName(t *testing.T) {
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), LibraryDir: t.TempDir(), PendingDir: t.TempDir(), Target: render.TargetOpenCode, Name: "a/b"})
	if err == nil || !strings.Contains(err.Error(), "invalid skill name") {
		t.Fatalf("expected invalid-name error, got %v", err)
	}
}

func TestApplyPendingCopyFailureLeavesLibraryUntouched(t *testing.T) {
	pending := t.TempDir()
	stage := filepath.Join(pending, "opencode", "skillx")
	if err := os.MkdirAll(stage, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stage, pullManifestFile), []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	unreadable := filepath.Join(stage, "secret.txt")
	if err := os.WriteFile(unreadable, []byte("x"), 0o000); err != nil {
		t.Fatal(err)
	}
	lib := t.TempDir()
	libTarget := filepath.Join(lib, "skillx")
	if err := os.MkdirAll(libTarget, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(libTarget, "SKILL.md"), []byte("---\nname: skillx\n---\n\noriginal\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), LibraryDir: lib, PendingDir: pending, Target: render.TargetOpenCode, Name: "skillx"})
	if err == nil {
		t.Fatal("expected copy failure")
	}
	if got, rerr := os.ReadFile(filepath.Join(libTarget, "SKILL.md")); rerr != nil || !strings.Contains(string(got), "original") {
		t.Fatalf("library must be untouched after copy failure: %q %v", got, rerr)
	}
}

func TestPullFrontmatterPropagatesOverlayError(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pferr", "")
	bundle = writeFrontmatterToml(t, bundle, "not [ valid toml\n")
	result := newPullResult()
	if err := pullFrontmatter(map[string]any{}, map[string]any{}, bundle, render.TargetOpenCode, result); err == nil {
		t.Fatal("expected overlay parse error propagation")
	}
}

func TestPullFrontmatterOwnedMetadataMatchesOverlay(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pfmatch", "")
	bundle = writeFrontmatterToml(t, bundle, "[metadata]\nowner = \"team\"\n")
	// source differs from installed, but installed equals the overlay value:
	// the harness just restored the overlay baseline — no refusal.
	source := map[string]any{"name": "pfmatch", "metadata": map[string]any{"owner": "different"}}
	installed := map[string]any{"name": "pfmatch", "metadata": map[string]any{"owner": "team"}}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatalf("unexpected refusal: %v", err)
	}
	if len(result.Refusals) != 0 {
		t.Fatalf("expected no refusals, got %v", result.Refusals)
	}
}

func TestPullFrontmatterDeletesRemovedTopLevelKey(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pftopdel", "")
	source := map[string]any{"name": "pftopdel", "old-key": "v1"}
	installed := map[string]any{"name": "pftopdel"}
	result := newPullResult()
	if err := pullFrontmatter(source, installed, bundle, render.TargetOpenCode, result); err != nil {
		t.Fatal(err)
	}
	if _, ok := source["old-key"]; ok {
		t.Fatalf("deleted top-level key still present: %v", source)
	}
	found := false
	for _, c := range result.FrontmatterChanges {
		if c.Key == "old-key" && c.To == nil {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected deletion change for old-key: %+v", result.FrontmatterChanges)
	}
}

func TestOverlayFrontmatterKeysCustomOverlayDir(t *testing.T) {
	if err := render.RegisterCustomTargets([]render.CustomTargetSpec{
		{Name: "customov", SkillRootUser: "/tmp/customov-root", OverlayDir: "my-overlays"},
	}); err != nil {
		t.Fatal(err)
	}
	bundle := loadBundleWithManifest(t, "fmcov", "")
	dir := filepath.Join(bundle.Root, "overlays", "my-overlays")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "frontmatter.toml"), []byte("allowed-tools = [\"git\"]\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	reloaded, err := skill.LoadBundle(bundle.Root)
	if err != nil {
		t.Fatal(err)
	}
	owned, _, err := overlayFrontmatterKeys(reloaded, render.Target("customov"))
	if err != nil {
		t.Fatal(err)
	}
	if !owned["allowed-tools"] {
		t.Errorf("expected custom overlay dir keys owned, got %v", owned)
	}
}

func TestPullBodyPropagatesOverlayTextError(t *testing.T) {
	bundle := loadBundleWithManifest(t, "pberr", `
[skill]
name = "pberr"

[targets.opencode]
prepend = "../escape.md"
`)
	result := newPullResult()
	out := ""
	err := pullBody("body\n", "body\n", bundle, render.TargetOpenCode, result, &out)
	if err == nil || !strings.Contains(err.Error(), "escapes skill root") {
		t.Fatalf("expected overlay escape error, got %v", err)
	}
}

func TestApplyPendingTmpCleanupFailure(t *testing.T) {
	pending := t.TempDir()
	stage := filepath.Join(pending, "opencode", "skillx")
	if err := os.MkdirAll(stage, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(stage, pullManifestFile), []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	lib := t.TempDir()
	libTarget := filepath.Join(lib, "skillx")
	if err := os.MkdirAll(libTarget, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(libTarget, "SKILL.md"), []byte("---\nname: skillx\n---\n\noriginal\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	// Pre-create the exact tmp path ApplyPending will try to clean; a
	// write-protected directory makes RemoveAll fail deterministically.
	tmp := filepath.Join(lib, "skillx.pull-tmp-"+fmt.Sprint(os.Getpid()))
	if err := os.MkdirAll(filepath.Join(tmp, "inner"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(tmp, 0o555); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(tmp, 0o755)
	err := ApplyPending(PullOptions{HomeDir: t.TempDir(), LibraryDir: lib, PendingDir: pending, Target: render.TargetOpenCode, Name: "skillx"})
	if err == nil {
		t.Fatal("expected tmp cleanup failure")
	}
	if got, rerr := os.ReadFile(filepath.Join(libTarget, "SKILL.md")); rerr != nil || !strings.Contains(string(got), "original") {
		t.Fatalf("library must be untouched after cleanup failure: %q %v", got, rerr)
	}
}

func TestPullSkipsEscapingSymlink(t *testing.T) {
	home, lib := t.TempDir(), t.TempDir()
	name := "escape"
	src := filepath.Join(lib, name)
	if err := os.MkdirAll(src, 0o755); err != nil {
		t.Fatal(err)
	}
	writeSkill(t, src, name, "portable")
	bundle, err := skill.LoadBundle(src)
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) > 0 {
		t.Fatal(errs[0])
	}
	if _, err := Install(RenderedSkill{Target: render.TargetOpenCode, Name: name, Path: rendered[0].Path}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err != nil {
		t.Fatal(err)
	}
	dest, err := InstallPath(render.TargetOpenCode, name, Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(home, "secret.txt")
	if err := os.WriteFile(outside, []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(dest, "references"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(dest, "references", "leak.md")); err != nil {
		t.Fatal(err)
	}

	result, err := Pull(PullOptions{HomeDir: home, Scope: render.ScopeUser, LibraryDir: lib, Target: render.TargetOpenCode, Name: name})
	if err != nil {
		t.Fatalf("escaping symlink should be skipped, not materialized: %v", err)
	}
	if result.StagePath == "" {
		t.Fatalf("pull must leave a stage tree for the apply pipeline: %+v", result)
	}
	if _, err := os.Stat(filepath.Join(result.StagePath, "references", "leak.md")); !os.IsNotExist(err) {
		t.Fatalf("escaping symlink target was materialized into pending tree: %v", err)
	}
}

func TestApplyPendingPreservesRepositoryAndCommits(t *testing.T) {
	opts, result := installAndStage(t, "preserve-repo")
	target := filepath.Join(opts.LibraryDir, "preserve-repo")
	if _, err := os.Stat(filepath.Join(result.StagePath, ".git")); !os.IsNotExist(err) {
		t.Fatalf("pending tree must not contain .git: %v", err)
	}
	if _, err := vcs.Init(target); err != nil {
		t.Fatal(err)
	}
	before, err := vcs.Head(target)
	if err != nil {
		t.Fatal(err)
	}
	if err := ApplyPending(opts); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(target, ".git")); err != nil {
		t.Fatalf("library repository was not preserved: %v", err)
	}
	after, err := vcs.Head(target)
	if err != nil {
		t.Fatal(err)
	}
	if before == after {
		t.Fatal("applying pending pull must create a forward commit")
	}
	history, err := vcs.History(target, 5)
	if err != nil {
		t.Fatal(err)
	}
	if len(history) == 0 || !strings.HasPrefix(history[0].Subject, "pull:") {
		t.Fatalf("expected pull commit at repository head, got %+v", history)
	}
}
