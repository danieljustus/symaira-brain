package render

import (
	"crypto/sha256"
	"encoding/hex"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// hashTree returns a stable digest of a directory tree (relative path,
// content and permission bits), used to prove byte-identical trees.
func hashTree(t *testing.T, root string) string {
	t.Helper()
	h := sha256.New()
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		h.Write([]byte(rel))
		h.Write([]byte{0})
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		h.Write([]byte{byte(info.Mode().Perm())})
		h.Write([]byte{0})
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		h.Write(data)
		h.Write([]byte{0})
		return nil
	})
	if err != nil {
		t.Fatalf("hashTree(%s): %v", root, err)
	}
	return hex.EncodeToString(h.Sum(nil))
}

func TestStagingRenderLeavesRenderDirUntouched(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: staging
description: Staging render test.
---

Body.
`)
	writeFile(t, filepath.Join(root, "scripts", "helper.sh"), "#!/bin/sh\n")

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// RenderDir is written exactly like the render command does.
	renderDir := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, renderDir, []Target{TargetOpenCode})
	if len(errs) != 0 || len(results) != 1 {
		t.Fatalf("RenderAll: results=%d errs=%v", len(results), errs)
	}
	before := hashTree(t, renderDir)

	// A comparison render must not touch RenderDir.
	staged, cleanup, err := StagingRender(bundle, []Target{TargetOpenCode})
	if err != nil {
		t.Fatalf("StagingRender: %v", err)
	}
	if len(staged) != 1 {
		t.Fatalf("want 1 staged render, got %d", len(staged))
	}
	if staged[0].Path == results[0].Path {
		t.Fatalf("staging render must not reuse the render dir: %q", staged[0].Path)
	}
	if !strings.Contains(staged[0].Path, "symskills-staging-") {
		t.Fatalf("staged path %q does not look like a staging directory", staged[0].Path)
	}
	after := hashTree(t, renderDir)
	if before != after {
		t.Fatalf("RenderDir changed by staging render:\nbefore: %s\nafter:  %s", before, after)
	}

	// The staging directory disappears with the cleanup function.
	stagingRoot := filepath.Dir(filepath.Dir(staged[0].Path))
	cleanup()
	if _, err := os.Stat(stagingRoot); !os.IsNotExist(err) {
		t.Fatalf("staging directory survived cleanup, stat err=%v", err)
	}
}

func TestStagingRenderRemovesDirOnFailedRender(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: staging-fail
description: Staging failure test.
---

Body.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "staging-fail"
version = "0.1.0"

[targets.opencode]
enabled = false
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// Point the staging factory at a known directory so the test can prove
	// it is removed on the error path.
	known := filepath.Join(t.TempDir(), "staging-fail-dir")
	orig := stagingMkdirTemp
	stagingMkdirTemp = func(dir, pattern string) (string, error) {
		return known, nil
	}
	defer func() { stagingMkdirTemp = orig }()

	_, cleanup, err := StagingRender(bundle, []Target{TargetOpenCode})
	if err == nil {
		t.Fatal("expected a render error for a disabled target")
	}
	if cleanup == nil {
		t.Fatal("cleanup must be non-nil even on error")
	}
	if _, statErr := os.Stat(known); !os.IsNotExist(statErr) {
		t.Fatalf("staging directory must not survive a failed render, stat err=%v", statErr)
	}
	// The no-op cleanup must be safe to call.
	cleanup()
}

func TestStagingRenderMkdirTempFailure(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: staging-tmp
description: MkdirTemp failure test.
---

Body.
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	orig := stagingMkdirTemp
	stagingMkdirTemp = func(dir, pattern string) (string, error) {
		return "", fs.ErrPermission
	}
	defer func() { stagingMkdirTemp = orig }()

	if _, cleanup, err := StagingRender(bundle, []Target{TargetOpenCode}); err == nil {
		t.Fatal("expected MkdirTemp failure to surface")
	} else if cleanup == nil {
		t.Fatal("cleanup must be non-nil even when the temp dir cannot be created")
	}
}

func TestCachedStagingRenderReusesUnchangedBundle(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: cached\ndescription: Cached render test.\n---\n\nBody.\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	cacheRoot := t.TempDir()
	first, cleanup, err := CachedStagingRender(bundle, []Target{TargetOpenCode}, cacheRoot)
	if err != nil {
		t.Fatal(err)
	}
	cleanup()
	if len(first) != 1 {
		t.Fatalf("first render count: got %d", len(first))
	}
	info, err := os.Stat(first[0].Path)
	if err != nil {
		t.Fatal(err)
	}
	second, cleanup, err := CachedStagingRender(bundle, []Target{TargetOpenCode}, cacheRoot)
	if err != nil {
		t.Fatal(err)
	}
	cleanup()
	if len(second) != 1 || second[0].Path != first[0].Path {
		t.Fatalf("cache miss: first=%#v second=%#v", first, second)
	}
	secondInfo, err := os.Stat(second[0].Path)
	if err != nil {
		t.Fatal(err)
	}
	if !secondInfo.ModTime().Equal(info.ModTime()) {
		t.Fatalf("cached render directory was rewritten: first=%v second=%v", info.ModTime(), secondInfo.ModTime())
	}
}
