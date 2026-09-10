//go:build aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package harness

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"golang.org/x/sys/unix"
)

func openConfigFile(path string) (*os.File, error) {
	dirFD, err := openDirectoryNoFollow(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	fileFD, err := unix.Openat(dirFD, filepath.Base(path), unix.O_RDONLY|unix.O_NONBLOCK|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	unix.Close(dirFD)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	return os.NewFile(uintptr(fileFD), path), nil
}

// openDirectoryNoFollow retains a directory descriptor after resolving every
// component with O_NOFOLLOW. This prevents a symlinked harness config parent
// from redirecting reads outside the configured tree.
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
			return -1, fmt.Errorf("configuration directory contains parent component")
		}
		next, err := unix.Openat(fd, component, flags, 0)
		unix.Close(fd)
		if err != nil {
			return -1, fmt.Errorf("open configuration directory component %q: %w", component, err)
		}
		fd = next
	}
	return fd, nil
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
