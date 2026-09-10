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
