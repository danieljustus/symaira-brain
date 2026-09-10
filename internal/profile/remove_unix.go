//go:build aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package profile

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"golang.org/x/sys/unix"
)

// Remove removes a profile entry beneath the configured profiles directory
// without following symlinked or reparse-like parent components. The final
// unlink is performed through the retained directory descriptor, so replacing
// the profiles directory with a symlink cannot redirect deletion elsewhere.
func Remove(name string) error {
	path := Path(name)
	dirFD, err := openDirectoryNoFollow(filepath.Dir(path))
	if err != nil {
		return fmt.Errorf("remove %s: %w", path, err)
	}
	defer unix.Close(dirFD)

	base := filepath.Base(path)
	if err := unix.Unlinkat(dirFD, base, 0); err != nil {
		// Darwin reports EPERM rather than EISDIR for unlinkat on a
		// directory; attempting AT_REMOVEDIR is safe because both operations
		// address the same retained parent descriptor and final name.
		if removeDirErr := unix.Unlinkat(dirFD, base, unix.AT_REMOVEDIR); removeDirErr != nil {
			return &os.PathError{Op: "remove", Path: path, Err: err}
		}
	}
	return nil
}

func normalizeSystemAlias(path string) string {
	if runtime.GOOS != "darwin" {
		return path
	}
	for alias, real := range map[string]string{"/var": "/private/var", "/tmp": "/private/tmp"} {
		if path == alias || strings.HasPrefix(path, alias+string(filepath.Separator)) {
			return real + path[len(alias):]
		}
	}
	return path
}

func openDirectoryNoFollow(path string) (int, error) {
	path = normalizeSystemAlias(path)
	clean := filepath.Clean(path)
	anchor := "."
	remainder := clean
	if filepath.IsAbs(clean) {
		anchor = string(filepath.Separator)
		remainder = strings.TrimPrefix(clean, anchor)
	}
	flags := unix.O_RDONLY | unix.O_DIRECTORY | unix.O_CLOEXEC | unix.O_NOFOLLOW
	fd, err := unix.Open(anchor, flags, 0)
	if err != nil {
		return -1, err
	}
	for _, component := range strings.Split(remainder, string(filepath.Separator)) {
		if component == "" || component == "." {
			continue
		}
		if component == ".." {
			unix.Close(fd)
			return -1, fmt.Errorf("profile directory contains parent component")
		}
		next, err := unix.Openat(fd, component, flags, 0)
		unix.Close(fd)
		if err != nil {
			return -1, fmt.Errorf("open profile directory component %q: %w", component, err)
		}
		fd = next
	}
	return fd, nil
}
