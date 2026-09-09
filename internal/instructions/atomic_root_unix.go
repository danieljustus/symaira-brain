//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package instructions

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"golang.org/x/sys/unix"
)

const (
	initialXattrBuffer = 4096
	maxXattrBuffer     = 16 * 1024 * 1024
)

type unixAtomicParent struct {
	dir  *os.File
	path string
}

func openAtomicParent(trustedRoot, parentName string, createParent bool) (atomicParent, error) {
	dir, err := openDirectoryNoFollow(trustedRoot, createParent)
	if err != nil {
		return nil, err
	}
	current := &unixAtomicParent{dir: dir, path: filepath.Clean(trustedRoot)}
	if parentName == "." {
		return current, nil
	}
	for _, component := range splitParent(parentName) {
		if createParent {
			if err := unix.Mkdirat(int(current.dir.Fd()), component, 0o700); err != nil && err != unix.EEXIST {
				_ = current.Close()
				return nil, err
			}
		}
		fd, err := unix.Openat(int(current.dir.Fd()), component, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
		if err != nil {
			_ = current.Close()
			return nil, err
		}
		next := &unixAtomicParent{dir: os.NewFile(uintptr(fd), component), path: filepath.Join(current.path, component)}
		_ = current.Close()
		current = next
	}
	return current, nil
}

// openDirectoryNoFollow rejects a symlink at the trusted-root endpoint. When
// creation is needed, it retains an already-open parent capability and creates
// the missing endpoint below it; it never uses MkdirAll on the root pathname.
// Ancestors supplied by the caller (for example macOS's /var alias) may be
// pathname-resolved, but the trusted root itself and every newly-created
// component are opened with O_NOFOLLOW.
func openDirectoryNoFollow(rootPath string, create bool) (*os.File, error) {
	if rootPath == "" {
		rootPath = "."
	}
	clean, err := filepath.Abs(filepath.Clean(rootPath))
	if err != nil {
		return nil, err
	}
	// macOS exposes /var as a system alias to /private/var. Resolve only this
	// fixed OS alias; every caller-controlled component is still opened below
	// the physical anchor with O_NOFOLLOW.
	if runtime.GOOS == "darwin" && (clean == "/var" || strings.HasPrefix(clean, "/var/")) {
		clean = "/private" + clean
	}
	currentFD, err := unix.Open(string(filepath.Separator), unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	current := os.NewFile(uintptr(currentFD), string(filepath.Separator))
	components := strings.FieldsFunc(strings.TrimPrefix(clean, string(filepath.Separator)), func(r rune) bool {
		return r == '/' || r == filepath.Separator
	})
	for _, component := range components {
		fd, openErr := unix.Openat(int(current.Fd()), component, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
		if openErr != nil && create && openErr == unix.ENOENT {
			if mkdirErr := unix.Mkdirat(int(current.Fd()), component, 0o700); mkdirErr != nil && mkdirErr != unix.EEXIST {
				_ = current.Close()
				return nil, mkdirErr
			}
			fd, openErr = unix.Openat(int(current.Fd()), component, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
		}
		if openErr != nil {
			_ = current.Close()
			return nil, openErr
		}
		next := os.NewFile(uintptr(fd), filepath.Join(current.Name(), component))
		if err := current.Close(); err != nil {
			_ = next.Close()
			return nil, err
		}
		current = next
	}
	return current, nil
}

func splitParent(parent string) []string {
	parts := make([]string, 0, 2)
	start := 0
	for i := 0; i <= len(parent); i++ {
		if i == len(parent) || parent[i] == '/' {
			if i > start {
				parts = append(parts, parent[start:i])
			}
			start = i + 1
		}
	}
	return parts
}

func (p *unixAtomicParent) Close() error {
	return p.dir.Close()
}

func (p *unixAtomicParent) Lstat(name string) (atomicFileInfo, error) {
	var stat unix.Stat_t
	if err := unix.Fstatat(int(p.dir.Fd()), name, &stat, unix.AT_SYMLINK_NOFOLLOW); err != nil {
		return atomicFileInfo{}, err
	}
	return atomicFileInfo{
		mode:         unixFileMode(uint32(stat.Mode)),
		size:         stat.Size,
		modeBits:     uint32(stat.Mode) & 0o7777,
		preserveMode: true,
	}, nil
}

func unixFileMode(mode uint32) os.FileMode {
	result := os.FileMode(mode & 0o777)
	switch mode & unix.S_IFMT {
	case unix.S_IFREG:
	case unix.S_IFDIR:
		result |= os.ModeDir
	case unix.S_IFLNK:
		result |= os.ModeSymlink
	default:
		result |= os.ModeDevice
	}
	return result
}

func (p *unixAtomicParent) OpenRead(name string) (*os.File, error) {
	fd, err := unix.Openat(int(p.dir.Fd()), name, unix.O_RDONLY|unix.O_CLOEXEC|unix.O_NOFOLLOW|unix.O_NONBLOCK, 0)
	if err != nil {
		return nil, err
	}
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		_ = unix.Close(fd)
		return nil, err
	}
	if uint32(stat.Mode)&unix.S_IFMT != unix.S_IFREG {
		_ = unix.Close(fd)
		return nil, fmt.Errorf("%s is not a regular file", name)
	}
	return os.NewFile(uintptr(fd), name), nil
}

func (p *unixAtomicParent) CreateTemp(name string) (*os.File, error) {
	fd, err := unix.Openat(int(p.dir.Fd()), name, unix.O_WRONLY|unix.O_CREAT|unix.O_EXCL|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0o600)
	if err != nil {
		return nil, err
	}
	return os.NewFile(uintptr(fd), name), nil
}

func (p *unixAtomicParent) Remove(name string) error {
	return unix.Unlinkat(int(p.dir.Fd()), name, 0)
}

func (p *unixAtomicParent) Rename(oldName, newName string) error {
	return unix.Renameat(int(p.dir.Fd()), oldName, int(p.dir.Fd()), newName)
}

func (p *unixAtomicParent) Sync() error {
	return p.dir.Sync()
}

func (p *unixAtomicParent) SetMode(file *os.File, mode uint32) error {
	return unix.Fchmod(int(file.Fd()), mode)
}

func (p *unixAtomicParent) CopyMetadata(name string, destination *os.File) error {
	// Metadata is read from the source inode through a no-follow descriptor.
	// Never chmod the source to work around a read-denied open: that mutates
	// user state and a failed restoration could leave permissions weakened.
	fd, err := unix.Openat(int(p.dir.Fd()), name, unix.O_RDONLY|unix.O_CLOEXEC|unix.O_NOFOLLOW|unix.O_NONBLOCK, 0)
	if err != nil {
		return fmt.Errorf("open metadata source %s without mutation: %w", name, err)
	}
	defer unix.Close(fd)
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return err
	}
	if uint32(stat.Mode)&unix.S_IFMT != unix.S_IFREG {
		return fmt.Errorf("%s is not a regular file", name)
	}
	if err := unix.Fchmod(int(destination.Fd()), uint32(stat.Mode)&0o7777); err != nil {
		return err
	}
	return copyXattrs(fd, int(destination.Fd()))
}

func copyXattrs(sourceFD, destinationFD int) error {
	list, err := readXattrBounded("extended attribute list", func(buffer []byte) (int, error) {
		return unix.Flistxattr(sourceFD, buffer)
	})
	if err != nil {
		return err
	}
	for start := 0; start < len(list); {
		end := start
		for end < len(list) && list[end] != 0 {
			end++
		}
		if end == start {
			start = end + 1
			continue
		}
		name := string(list[start:end])
		if name == "com.apple.provenance" {
			// macOS assigns this system provenance xattr and rejects copying it
			// to a replacement. It is not a user ACL or metadata contract.
			start = end + 1
			continue
		}
		value, err := readXattrBounded(fmt.Sprintf("extended attribute %q", name), func(buffer []byte) (int, error) {
			return unix.Fgetxattr(sourceFD, name, buffer)
		})
		if err != nil {
			return err
		}
		if err := unix.Fsetxattr(destinationFD, name, value, 0); err != nil {
			return fmt.Errorf("write extended attribute %q: %w", name, err)
		}
		start = end + 1
	}
	return nil
}

func readXattrBounded(kind string, read func([]byte) (int, error)) ([]byte, error) {
	capacity := initialXattrBuffer
	for {
		if err := validateXattrAllocation(kind, capacity); err != nil {
			return nil, err
		}
		buffer := make([]byte, capacity)
		n, err := read(buffer)
		if err == unix.ERANGE {
			if capacity == maxXattrBuffer {
				return nil, fmt.Errorf("%s exceeds %d-byte limit", kind, maxXattrBuffer)
			}
			capacity *= 2
			if capacity > maxXattrBuffer {
				capacity = maxXattrBuffer
			}
			continue
		}
		if err != nil {
			return nil, fmt.Errorf("read %s: %w", kind, err)
		}
		if n < 0 || n > len(buffer) {
			return nil, fmt.Errorf("read %s returned invalid size %d", kind, n)
		}
		return buffer[:n], nil
	}
}

func validateXattrAllocation(kind string, size int) error {
	if size < 0 || size > maxXattrBuffer {
		return fmt.Errorf("%s exceeds %d-byte limit", kind, maxXattrBuffer)
	}
	return nil
}
