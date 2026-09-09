package render

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func hardeningBundle(t *testing.T) (*skill.Bundle, Rendered) {
	t.Helper()
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: atomic-skill\ndescription: test\n---\nNew body.\n")
	writeFile(t, filepath.Join(root, "references", "guide.md"), "new support\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	item, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	return bundle, item
}

func TestWriteRenderedRollbackPreservesOldTreeOnEveryInjectedFailure(t *testing.T) {
	bundle, item := hardeningBundle(t)
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	oldSkill := []byte("old skill\n")
	oldSupport := []byte("old support\n")
	writeFile(t, filepath.Join(out, "SKILL.md"), string(oldSkill))
	writeFile(t, filepath.Join(out, "references", "guide.md"), string(oldSupport))

	for _, operation := range []string{"write", "sync-file", "sync-dir", "swap-backup", "swap-install", "swap-remove-backup"} {
		t.Run(operation, func(t *testing.T) {
			renderFaultHook = func(op, _ string) error {
				if op == operation {
					return os.ErrPermission
				}
				return nil
			}
			t.Cleanup(func() { renderFaultHook = nil })
			changed := item
			changed.SkillMD = strings.Replace(changed.SkillMD, "New body.", "Changed body.", 1)
			if err := writeRendered(bundle, out, changed, TargetOpenCode, sourceTreeHash(bundle)); err == nil {
				t.Fatalf("expected injected %s failure", operation)
			}
			if got, err := os.ReadFile(filepath.Join(out, "SKILL.md")); err != nil || string(got) != string(oldSkill) {
				t.Fatalf("old SKILL.md not restored: %q, %v", got, err)
			}
			if got, err := os.ReadFile(filepath.Join(out, "references", "guide.md")); err != nil || string(got) != string(oldSupport) {
				t.Fatalf("old support file not restored: %q, %v", got, err)
			}
		})
	}
}

func TestWriteRenderedRebuildsPoisonedManifestOutputs(t *testing.T) {
	bundle, item := hardeningBundle(t)
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	writeFile(t, filepath.Join(out, "references", "guide.md"), "poisoned\n")
	writeFile(t, filepath.Join(out, "extra.txt"), "unexpected\n")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	if got, _ := os.ReadFile(filepath.Join(out, "references", "guide.md")); string(got) != "new support\n" {
		t.Fatalf("modified output was reused: %q", got)
	}
	if _, err := os.Stat(filepath.Join(out, "extra.txt")); !os.IsNotExist(err) {
		t.Fatalf("extra output survived rebuild: %v", err)
	}
}

func TestWriteRenderedRejectsMarkerSymlinkWithoutFollowingIt(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	bundle, item := hardeningBundle(t)
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside-marker.json")
	if err := os.WriteFile(outside, []byte(`{"source_hash":"poison"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(out, ".symskills.json")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(out, ".symskills.json")); err != nil {
		t.Fatal(err)
	}
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Lstat(filepath.Join(out, ".symskills.json")); err != nil || info.Mode()&os.ModeSymlink != 0 {
		t.Fatalf("marker symlink was retained: %v", err)
	}
	if got, _ := os.ReadFile(outside); string(got) != `{"source_hash":"poison"}` {
		t.Fatalf("outside marker was modified: %q", got)
	}
}

func TestWriteRenderedKeepsNestedMarkerAsResource(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: nested-marker\ndescription: test\n---\nBody.\n")
	nested := []byte(`{"source_hash":"poison","output_manifest":"ignore"}`)
	writeFile(t, filepath.Join(root, "references", ".symskills.json"), string(nested))
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	item, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(filepath.Join(out, "references", ".symskills.json")); err != nil || string(got) != string(nested) {
		t.Fatalf("nested marker was not preserved: %q, %v", got, err)
	}
	var marker map[string]any
	markerBytes, err := os.ReadFile(filepath.Join(out, ".symskills.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(markerBytes, &marker); err != nil {
		t.Fatal(err)
	}
	if marker["source_hash"] == "poison" {
		t.Fatal("nested marker poisoned the root marker")
	}
}

func TestWriteRenderedRejectsOversizedPreservedMetadata(t *testing.T) {
	bundle, item := hardeningBundle(t)
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	metadata := strings.Repeat("x", skill.MaxInputSize-256)
	marker := fmt.Sprintf(`{"metadata":%q}`, metadata)
	writeFile(t, filepath.Join(out, ".symskills.json"), marker)
	changed := item
	changed.SkillMD = strings.Replace(item.SkillMD, "New body.", "Changed body.", 1)
	if err := writeRendered(bundle, out, changed, TargetOpenCode, sourceTreeHash(bundle)); err == nil {
		t.Fatal("expected oversized preserved metadata to be rejected")
	}
	if got, err := os.ReadFile(filepath.Join(out, "SKILL.md")); err != nil || string(got) != item.SkillMD {
		t.Fatalf("old tree was not preserved: %q, %v", got, err)
	}
}

func TestWriteRenderedResistsDestinationSymlinkSwap(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	bundle, item := hardeningBundle(t)
	parent := t.TempDir()
	out := filepath.Join(parent, "rendered")
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0o755); err != nil {
		t.Fatal(err)
	}
	changed := item
	changed.SkillMD = strings.Replace(item.SkillMD, "New body.", "Changed body.", 1)
	renderFaultHook = func(operation, _ string) error {
		if operation == "swap-install" {
			if err := os.Symlink(outside, out); err != nil {
				return err
			}
		}
		return nil
	}
	t.Cleanup(func() { renderFaultHook = nil })
	if err := writeRendered(bundle, out, changed, TargetOpenCode, sourceTreeHash(bundle)); err == nil {
		t.Fatal("expected destination symlink swap to be rejected")
	}
	if _, err := os.Stat(filepath.Join(outside, "SKILL.md")); !os.IsNotExist(err) {
		t.Fatalf("symlink swap escaped into outside tree: %v", err)
	}
	if info, err := os.Lstat(out); err != nil || info.Mode()&os.ModeSymlink == 0 {
		t.Fatalf("destination swap did not remain confined to the symlink path: %v", err)
	}
}

func TestWriteRenderedSerializesConcurrentSameDestination(t *testing.T) {
	bundle, item := hardeningBundle(t)
	const workers = 8
	const rounds = 16
	for round := 0; round < rounds; round++ {
		out := filepath.Join(t.TempDir(), fmt.Sprintf("rendered-%d", round))
		start := make(chan struct{})
		errs := make(chan error, workers)
		var wg sync.WaitGroup
		for i := 0; i < workers; i++ {
			wg.Add(1)
			go func(i int) {
				defer wg.Done()
				<-start
				candidate := item
				candidate.SkillMD = fmt.Sprintf("%s<!-- concurrent-%d-%d -->\n", item.SkillMD, round, i)
				errs <- writeRendered(bundle, out, candidate, TargetOpenCode, sourceTreeHash(bundle))
			}(i)
		}
		close(start)
		wg.Wait()
		close(errs)
		for err := range errs {
			if err != nil {
				t.Fatal(err)
			}
		}
		got, err := os.ReadFile(filepath.Join(out, "SKILL.md"))
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(string(got), fmt.Sprintf("<!-- concurrent-%d-", round)) {
			t.Fatalf("round %d final output is not one complete concurrent render: %q", round, got)
		}
	}
}

func TestLockDestinationRemainsBounded(t *testing.T) {
	out := filepath.Join(t.TempDir(), "rendered")
	root, err := openDestinationRoot(filepath.Dir(out))
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	unlock, err := lockDestination(root, ".", filepath.Base(out))
	if err != nil {
		t.Fatal(err)
	}
	defer unlock()
	previous := renderLockTimeout
	renderLockTimeout = 10 * time.Millisecond
	t.Cleanup(func() { renderLockTimeout = previous })
	start := time.Now()
	reset, err := lockDestination(root, ".", filepath.Base(out))
	if err == nil {
		if reset != nil {
			reset()
		}
		t.Fatal("expected lock contention to time out")
	}
	if !strings.Contains(err.Error(), "timed out acquiring render lock") {
		t.Fatalf("unexpected lock contention error: %v", err)
	}
	if elapsed := time.Since(start); elapsed > time.Second {
		t.Fatalf("contention test took too long: %v", elapsed)
	}
}
