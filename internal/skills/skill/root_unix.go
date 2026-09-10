//go:build unix

package skill

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

func openRootFile(root *os.Root, name string) (*os.File, error) {
	return root.OpenFile(name, os.O_RDONLY|syscall.O_NONBLOCK|syscall.O_NOFOLLOW, 0)
}

func openRootChildNoFollow(parent *os.Root, name string) (*os.Root, error) {
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

func openRootPathNoFollow(root *os.Root, rel string, create bool) (*os.Root, error) {
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
			return nil, fmt.Errorf("unsafe path component %q", name)
		}
		child, err := openRootChildNoFollow(current, name)
		if os.IsNotExist(err) && create {
			if mkdirErr := current.Mkdir(name, 0o755); mkdirErr != nil && !os.IsExist(mkdirErr) {
				if owned {
					_ = current.Close()
				}
				return nil, mkdirErr
			}
			child, err = openRootChildNoFollow(current, name)
		}
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

func writeRootFileNoFollow(root *os.Root, name string, data []byte, mode os.FileMode) error {
	file, err := root.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_TRUNC|syscall.O_NOFOLLOW, mode)
	if err != nil {
		return err
	}
	_, writeErr := file.Write(data)
	closeErr := file.Close()
	if writeErr != nil {
		return writeErr
	}
	return closeErr
}
