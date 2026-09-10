package skill

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
)

func writeMinimalSkill(t *testing.T, root string) {
	t.Helper()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: safe-skill\ndescription: test\n---\nBody\n")
}

func writeBytes(t *testing.T, path string, content []byte) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestLoadBundleRejectsControlFileSymlinks(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err := os.WriteFile(outside, []byte("---\nname: safe-skill\ndescription: outside\n---\nBody\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "SKILL.md")); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "path escapes from parent") {
		t.Fatalf("expected control-file symlink rejection, got %v", err)
	}
}

func TestLoadBundleRejectsEscapingResourceSymlink(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	writeMinimalSkill(t, root)
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err := os.WriteFile(outside, []byte("outside\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "references.md")); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "escapes skill root") {
		t.Fatalf("expected resource symlink escape rejection, got %v", err)
	}
}

func TestLoadBundleTraversesInternalSymlinkDirectories(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(filepath.Join(root, "real"), 0o755); err != nil {
		t.Fatal(err)
	}
	writeMinimalSkill(t, root)
	writeFile(t, filepath.Join(root, "real", "inner.txt"), "content")
	if err := os.Symlink("real", filepath.Join(root, "linked")); err != nil {
		t.Fatal(err)
	}
	bundle, err := LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	found := false
	for _, resource := range bundle.Resources {
		if resource.Path == "linked/inner.txt" {
			found = true
		}
	}
	if !found {
		t.Fatalf("internal symlink directory was not inventoried: %+v", bundle.Resources)
	}
	dst := filepath.Join(t.TempDir(), "rendered")
	if err := CopyBundleSupport(bundle, dst); err != nil {
		t.Fatalf("CopyBundleSupport: %v", err)
	}
	data, err := os.ReadFile(filepath.Join(dst, "linked", "inner.txt"))
	if err != nil {
		t.Fatalf("copied symlink-directory file: %v", err)
	}
	if string(data) != "content" {
		t.Fatalf("copied content: %q", data)
	}
}

func TestLoadBundleRejectsOverlayDirectoryEscape(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	writeMinimalSkill(t, root)
	outside := filepath.Join(t.TempDir(), "outside-overlays")
	if err := os.MkdirAll(filepath.Join(outside, "hermes", "blocks"), 0o755); err != nil {
		t.Fatal(err)
	}
	writeFile(t, filepath.Join(outside, "hermes", "blocks", "worker.md"), "outside\n")
	if err := os.Symlink(outside, filepath.Join(root, "overlays")); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "escapes") {
		t.Fatalf("expected overlay directory escape rejection, got %v", err)
	}
}

func TestLoadBundleRejectsInputAndFrontmatterLimits(t *testing.T) {
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	writeBytes(t, filepath.Join(root, "SKILL.md"), append([]byte("---\nname: safe-skill\ndescription: test\n---\n"), bytes.Repeat([]byte{'x'}, MaxInputSize)...))
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "maximum input size") {
		t.Fatalf("expected SKILL.md input limit rejection, got %v", err)
	}

	frontmatter := bytes.Repeat([]byte{'x'}, MaxFrontmatterSize+1)
	content := append([]byte("---\nname: safe-skill\ndescription: "), frontmatter...)
	content = append(content, []byte("\n---\nBody\n")...)
	writeBytes(t, filepath.Join(root, "SKILL.md"), content)
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "frontmatter exceeds maximum size") {
		t.Fatalf("expected frontmatter limit rejection, got %v", err)
	}
}

func TestLoadBundleRejectsResourceTreeEntryLimit(t *testing.T) {
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	writeMinimalSkill(t, root)
	for i := 0; i < MaxResourceEntries; i++ {
		writeFile(t, filepath.Join(root, "files", "resource-"+strconv.Itoa(i)), "")
	}
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "maximum entry count") {
		t.Fatalf("expected resource entry limit rejection, got %v", err)
	}
}

func TestLoadBundleRejectsExcessiveResourceDepth(t *testing.T) {
	root := filepath.Join(t.TempDir(), "safe-skill")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	writeMinimalSkill(t, root)
	deep := root
	for i := 0; i <= MaxResourceDepth; i++ {
		deep = filepath.Join(deep, "d")
	}
	writeFile(t, filepath.Join(deep, "file.txt"), "content")
	if _, err := LoadBundle(root); err == nil || !strings.Contains(err.Error(), "maximum depth") {
		t.Fatalf("expected resource depth limit rejection, got %v", err)
	}
}
