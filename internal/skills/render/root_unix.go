//go:build unix

package render

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

// openDestinationRootSecure creates/opens every component from a retained
// filesystem root. O_NOFOLLOW is applied to the component actually opened;
// checking a pathname and reopening it later would leave a symlink race.
func openDestinationRootSecure(path string) (*os.Root, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	abs = normalizeSystemAlias(abs)
	root, err := os.OpenRoot(string(filepath.Separator))
	if err != nil {
		return nil, err
	}
	parts := strings.Split(strings.TrimPrefix(abs, string(filepath.Separator)), string(filepath.Separator))
	for _, part := range parts {
		if part == "" {
			continue
		}
		child, openErr := openRenderChildRootNoFollow(root, part)
		if os.IsNotExist(openErr) {
			if mkdirErr := root.Mkdir(part, 0o755); mkdirErr != nil && !os.IsExist(mkdirErr) {
				_ = root.Close()
				return nil, mkdirErr
			}
			child, openErr = openRenderChildRootNoFollow(root, part)
		}
		if openErr != nil {
			_ = root.Close()
			return nil, fmt.Errorf("open destination component %q: %w", part, openErr)
		}
		_ = root.Close()
		root = child
	}
	return root, nil
}

func openRenderChildRootNoFollow(parent *os.Root, name string) (*os.Root, error) {
	file, err := parent.OpenFile(name, os.O_RDONLY|syscall.O_DIRECTORY|syscall.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	child, err := os.OpenRoot(fmt.Sprintf("/dev/fd/%d", file.Fd()))
	closeErr := file.Close()
	if err != nil {
		return nil, err
	}
	if closeErr != nil {
		_ = child.Close()
		return nil, closeErr
	}
	return child, nil
}

func openRenderDirNoFollow(root *os.Root, rel string) (*os.Root, error) {
	if rel == "." || rel == "" {
		return root, nil
	}
	current := root
	owned := false
	for _, name := range strings.Split(filepath.Clean(rel), string(filepath.Separator)) {
		if name == "" || name == "." {
			continue
		}
		if name == ".." {
			if owned {
				_ = current.Close()
			}
			return nil, fmt.Errorf("unsafe rendered path component %q", name)
		}
		child, err := openRenderChildRootNoFollow(current, name)
		if err != nil {
			if owned {
				_ = current.Close()
			}
			return nil, err
		}
		if owned {
			_ = current.Close()
		}
		current, owned = child, true
	}
	return current, nil
}

func openRenderFileNoFollow(root *os.Root, path string) (*os.File, error) {
	parent, err := openRenderDirNoFollow(root, filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	if parent != root {
		defer parent.Close()
	}
	return parent.OpenFile(filepath.Base(path), os.O_RDONLY|syscall.O_NOFOLLOW, 0)
}

func normalizeSystemAlias(path string) string {
	for _, alias := range []struct{ name, real string }{
		{name: "/var", real: "/private/var"},
		{name: "/tmp", real: "/private/tmp"},
	} {
		if path == alias.name || strings.HasPrefix(path, alias.name+"/") {
			if target, err := os.Readlink(alias.name); err == nil && target == alias.real {
				return alias.real + strings.TrimPrefix(path, alias.name)
			}
		}
	}
	return path
}
