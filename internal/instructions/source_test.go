package instructions

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestNewSource_HonorsAbsoluteXDGConfigHome(t *testing.T) {
	home := t.TempDir()
	xdg := filepath.Join(t.TempDir(), "config")
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", xdg)

	source := NewSource("")
	want := filepath.Join(xdg, "symbrain", GlobalFileName)
	if source.GlobalPath != want {
		t.Fatalf("GlobalPath = %q, want %q", source.GlobalPath, want)
	}
}

func TestSourceRejectsSymlink(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "outside.md")
	link := filepath.Join(root, "instructions.md")
	if err := os.WriteFile(target, []byte("outside\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, link); err != nil {
		if runtime.GOOS == "windows" {
			t.Skipf("symlink unavailable: %v", err)
		}
		t.Fatal(err)
	}

	_, err := (&Source{GlobalPath: link}).Content()
	if err == nil || (!strings.Contains(err.Error(), "regular file") && !strings.Contains(err.Error(), "symlink")) {
		t.Fatalf("Content() error = %v, want symlink rejection", err)
	}
}

func TestSourceRejectsDirectory(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "instructions.md")
	if err := os.Mkdir(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	_, err := (&Source{GlobalPath: dir}).Content()
	if err == nil || !strings.Contains(err.Error(), "regular file") {
		t.Fatalf("Content() error = %v, want regular-file rejection", err)
	}
}

func TestSourceRejectsOversizedFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "instructions.md")
	if err := os.WriteFile(path, make([]byte, MaxSourceFileBytes+1), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err := (&Source{GlobalPath: path}).Content()
	if err == nil || !strings.Contains(err.Error(), "maximum size") {
		t.Fatalf("Content() error = %v, want size rejection", err)
	}
}

func TestSourceRejectsOversizedMergedContent(t *testing.T) {
	root := t.TempDir()
	global := filepath.Join(root, "global.md")
	project := filepath.Join(root, "project.md")
	if err := os.WriteFile(global, make([]byte, MaxSourceFileBytes), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(project, make([]byte, MaxSourceTotalBytes-MaxSourceFileBytes+1), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err := (&Source{GlobalPath: global, ProjectPath: project}).Content()
	if err == nil || !strings.Contains(err.Error(), "merged source") {
		t.Fatalf("Content() error = %v, want merged-size rejection", err)
	}
}

func TestWriteFileAtomicPreservesModeAndUses0600ForNewFiles(t *testing.T) {
	root := t.TempDir()
	existing := filepath.Join(root, "existing.md")
	if err := os.WriteFile(existing, []byte("old"), 0o640); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "existing.md", []byte("new")); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(existing); err != nil || string(got) != "new" {
		t.Fatalf("existing content = %q, err = %v", got, err)
	}
	if runtime.GOOS != "windows" {
		info, err := os.Stat(existing)
		if err != nil {
			t.Fatal(err)
		}
		if got := info.Mode().Perm(); got != 0o640 {
			t.Fatalf("existing mode = %o, want 640", got)
		}

		modeZero := filepath.Join(root, "mode-zero.md")
		if err := os.WriteFile(modeZero, []byte("old"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Chmod(modeZero, 0); err != nil {
			t.Fatal(err)
		}
		if err := WriteFileAtomic(root, "mode-zero.md", []byte("new")); err == nil {
			t.Fatal("mode-zero source was made readable by mutating it")
		}
		info, err = os.Stat(modeZero)
		if err != nil {
			t.Fatal(err)
		}
		if got := info.Mode().Perm(); got != 0 {
			t.Fatalf("mode-zero existing mode = %o, want 0", got)
		}
	}

	fresh := filepath.Join(root, "fresh.md")
	if err := WriteFileAtomic(root, "fresh.md", []byte("fresh")); err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" {
		info, err := os.Stat(fresh)
		if err != nil {
			t.Fatal(err)
		}
		if got := info.Mode().Perm(); got != 0o600 {
			t.Fatalf("fresh mode = %o, want 600", got)
		}
	}
}

func TestWriteFileAtomicRejectsSymlinkedParent(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink privileges are not available on every Windows runner")
	}
	root := t.TempDir()
	outside := t.TempDir()
	link := filepath.Join(root, "rules")
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "rules/instructions.md", []byte("must not escape")); err == nil {
		t.Fatal("WriteFileAtomic followed a symlinked parent")
	}
	if _, err := os.Stat(filepath.Join(outside, "instructions.md")); !os.IsNotExist(err) {
		t.Fatalf("outside target exists after rejected write: %v", err)
	}
}

func TestWriteFileAtomicRejectsSymlinkAndSpecialTargets(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("special-file setup is not portable to Windows")
	}
	root := t.TempDir()
	outside := filepath.Join(root, "outside.md")
	link := filepath.Join(root, "link.md")
	if err := os.WriteFile(outside, []byte("outside"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "link.md", []byte("must not follow")); err == nil {
		t.Fatal("WriteFileAtomic replaced a symlink target")
	}
	got, err := os.ReadFile(outside)
	if err != nil || string(got) != "outside" {
		t.Fatalf("outside symlink target changed: %q, %v", got, err)
	}

	directory := filepath.Join(root, "directory")
	if err := os.Mkdir(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "directory", []byte("must reject directory")); err == nil {
		t.Fatal("WriteFileAtomic replaced a directory")
	}
}

// TestWriteFileAtomicCreatesNestedParentAndLeavesNoTempFile exercises the
// capability walk and the cleanup path after a successful rename.
func TestWriteFileAtomicCreatesNestedParentAndLeavesNoTempFile(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "one", "two", "instructions.md")
	if err := WriteFileAtomic(root, "one/two/instructions.md", []byte("durable")); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(path); err != nil || string(got) != "durable" {
		t.Fatalf("written content = %q, err = %v", got, err)
	}
	matches, err := filepath.Glob(filepath.Join(filepath.Dir(path), ".instructions.md.symbrain-tmp-*"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 0 {
		t.Fatalf("atomic temp files remain: %v", matches)
	}
}

func TestWriteFileAtomicRejectsSymlinkedParentInsideRoot(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink privileges are not available on every Windows runner")
	}
	root := t.TempDir()
	real := filepath.Join(root, "real")
	if err := os.Mkdir(real, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(real, filepath.Join(root, "rules")); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "rules/instructions.md", []byte("must reject")); err == nil {
		t.Fatal("WriteFileAtomic followed an in-root symlinked parent")
	}
	if _, err := os.Stat(filepath.Join(real, "instructions.md")); !os.IsNotExist(err) {
		t.Fatalf("in-root symlink target exists after rejected write: %v", err)
	}
}

func TestAtomicFileRetainsParentCapabilityAfterReplacement(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("directory rename semantics differ on Windows")
	}
	root := t.TempDir()
	parent := filepath.Join(root, "rules")
	outside := t.TempDir()
	if err := os.Mkdir(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	file, err := OpenAtomicFile(root, "rules/instructions.md", false)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := os.Rename(parent, filepath.Join(root, "moved-rules")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, parent); err != nil {
		t.Fatal(err)
	}
	if err := file.Write([]byte("retained")); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(filepath.Join(root, "moved-rules", "instructions.md")); err != nil || string(got) != "retained" {
		t.Fatalf("retained target = %q, err = %v", got, err)
	}
	if _, err := os.Stat(filepath.Join(outside, "instructions.md")); !os.IsNotExist(err) {
		t.Fatalf("replacement parent received target: %v", err)
	}
}

func TestWriteFileAtomicRejectsUnsafeRelativeTargets(t *testing.T) {
	root := t.TempDir()
	for _, target := range []string{"", "/tmp/file", `\\server\\share\\file`, "../file", `..\\file`, "nested/../../file", "C:/file", "file/"} {
		if err := WriteFileAtomic(root, target, []byte("must reject")); err == nil {
			t.Fatalf("WriteFileAtomic accepted unsafe target %q", target)
		}
	}
}
