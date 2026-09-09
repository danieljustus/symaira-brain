//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package instructions

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"golang.org/x/sys/unix"
)

func TestWriteFileAtomicRejectsTrustedRootSymlink(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	link := filepath.Join(root, "root-link")
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(link, "instructions.md", []byte("must reject")); err == nil {
		t.Fatal("atomic write accepted a symlinked trusted root")
	}
	if _, err := os.Stat(filepath.Join(outside, "instructions.md")); !os.IsNotExist(err) {
		t.Fatalf("symlink destination was modified: %v", err)
	}
}

func TestAtomicReadDoesNotBlockOnFIFO(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("FIFO is Unix-only")
	}
	root := t.TempDir()
	fifo := filepath.Join(root, "instructions.md")
	if err := unix.Mkfifo(fifo, 0o600); err != nil {
		t.Fatal(err)
	}
	file, err := OpenAtomicFile(root, "instructions.md", false)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if _, err := file.ReadBounded(); err == nil {
		t.Fatal("FIFO was accepted as an instruction source")
	}
}

func TestWriteFileAtomicPreservesSpecialModeAndUserXattr(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "metadata.md")
	if err := os.WriteFile(target, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(target, 0o751|os.ModeSetuid); err != nil {
		t.Fatal(err)
	}
	xattrName := "user.symbrain.test"
	if runtime.GOOS == "darwin" {
		xattrName = "com.apple.metadata:_kMDItemUserTags"
	}
	if err := unix.Setxattr(target, xattrName, []byte("phase6"), 0); err != nil {
		t.Skipf("filesystem does not permit test xattrs: %v", err)
	}
	before, err := os.Stat(target)
	if err != nil {
		t.Fatal(err)
	}
	if err := WriteFileAtomic(root, "metadata.md", []byte("new")); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(target)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSetuid == 0 || info.Mode().Perm() != 0o751 {
		t.Fatalf("mode = %v, want setuid 751", info.Mode())
	}
	if info.Mode() != before.Mode() {
		t.Fatalf("replacement mode = %v, source mode before write = %v", info.Mode(), before.Mode())
	}
	value := make([]byte, 64)
	n, err := unix.Getxattr(target, xattrName, value)
	if err != nil {
		t.Fatal(err)
	}
	if string(value[:n]) != "phase6" {
		t.Fatalf("xattr = %q, want phase6", value[:n])
	}
}

func TestValidateXattrAllocationIsBounded(t *testing.T) {
	for _, size := range []int{0, 1, maxXattrBuffer} {
		if err := validateXattrAllocation("xattr", size); err != nil {
			t.Fatalf("size %d rejected: %v", size, err)
		}
	}
	for _, size := range []int{-1, maxXattrBuffer + 1} {
		if err := validateXattrAllocation("xattr", size); err == nil {
			t.Fatalf("size %d accepted", size)
		}
	}
}

func TestReadXattrBoundedRetriesGrowthAndReadsEmptyValue(t *testing.T) {
	calls := 0
	value, err := readXattrBounded("growing xattr", func(buffer []byte) (int, error) {
		calls++
		if calls == 1 {
			return 0, unix.ERANGE
		}
		copy(buffer, "grown")
		return len("grown"), nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if calls != 2 || string(value) != "grown" {
		t.Fatalf("calls=%d value=%q, want 2 and grown", calls, value)
	}

	emptyCalls := 0
	value, err = readXattrBounded("empty xattr", func([]byte) (int, error) {
		emptyCalls++
		return 0, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if emptyCalls != 1 || len(value) != 0 {
		t.Fatalf("calls=%d len=%d, want 1 and 0", emptyCalls, len(value))
	}
}
